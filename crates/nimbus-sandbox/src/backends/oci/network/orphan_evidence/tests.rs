use std::fs;
use std::path::PathBuf;
use std::process::Command;

use nimbus_network::{
    LocalNetworkAttachmentAuthority, LocalNetworkStateStore, NetworkAttachmentReservationState,
    NetworkAttachmentSegmentAssociation, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId,
    NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration, NetworkSegmentAllocator,
    NetworkSegmentId, NetworkStatePartition,
};
use tempfile::TempDir;

use super::*;
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
    host_managed_attachment_requirements,
};
use crate::backends::oci::network::attachment_lifecycle::{
    AttachmentBackendKind, OciAttachmentLifecycle,
};
use crate::backends::oci::network::dto::{IpamState, NetavarkProviderOperation};
use crate::backends::oci::network::ipam::{
    NetavarkTeardownPlan, OciIpamAuthority, OciIpamEvidenceLifecycle, begin_netavark_setup,
    begin_netavark_setup_execution, begin_netavark_teardown, begin_netavark_teardown_execution,
    complete_netavark_setup, complete_netavark_teardown, confirm_netavark_provider_detached,
};
use crate::backends::oci::network::provider_locator::OciAttachmentProviderKind;
use crate::backends::oci::network::{
    OciNetworkConfig, OciPlacementProvider, SingleNodeSegmentAllocator,
    default_network_attachment_id, place_sandbox_on_block,
};
use crate::instance::SandboxId;

struct EvidenceFixture {
    _temp_dir: TempDir,
    workload_root: PathBuf,
    network_root: PathBuf,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    layout: OciNetworkLayout,
    ipam: OciIpamAuthority,
    allocator: SingleNodeSegmentAllocator,
    attachments: LocalNetworkAttachmentAuthority,
    claim: NetworkReservationClaim,
    config: OciNetworkConfig,
}

impl EvidenceFixture {
    fn new(label: &str, backend: AttachmentBackendKind, desired_claim_substitution: bool) -> Self {
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
        let config = place_sandbox_on_block(
            &allocator,
            &ipam,
            &tenant_id,
            &layout,
            &sandbox_id,
            &claim,
            OciPlacementProvider::new(backend.provider_kind(), |segment, reservation_claim| {
                OciAttachmentLifecycle::config_from_segment(
                    backend,
                    PathBuf::from("netavark-not-executed"),
                    PathBuf::from("aardvark-not-executed"),
                    segment,
                    reservation_claim,
                )
            }),
        )
        .expect("placement should persist IPAM evidence before effects");
        let attachment_id = default_network_attachment_id(&sandbox_id);
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
        let registration_kind = match backend {
            AttachmentBackendKind::Container => SandboxAttachmentRegistrationKind::Container,
            AttachmentBackendKind::Krun => SandboxAttachmentRegistrationKind::Krun,
        };
        let plan = NetworkPlan::new(
            NetworkPlanId::for_tenant_workload_plan(&tenant_id, sandbox_id.as_str()),
            NetworkResourceGeneration::new(1),
            NetworkPlanContentDigest::sha256(format!("nnc5.2b:{label}")),
            host_managed_attachment_requirements(registration_kind),
        );
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

    fn publish_exact_artifacts(&self) {
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

    fn authority_bytes(&self) -> Vec<u8> {
        fs::read(LocalNetworkStateStore::authority_path_for(
            &self.network_root,
        ))
        .expect("authority bytes should read")
    }
}

fn reservation_claim(label: &str) -> NetworkReservationClaim {
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

fn netavark_attempt(
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

#[test]
fn deterministic_union_reopens_without_mutation_and_never_promotes_artifact_names() {
    let fixture = EvidenceFixture::new(
        "deterministic-union",
        AttachmentBackendKind::Container,
        false,
    );
    fixture.publish_exact_artifacts();
    let unmatched_netns = fixture
        .layout
        .netns_root
        .join("artifact-name-is-not-an-attachment");
    fs::write(&unmatched_netns, b"untrusted").expect("unmatched artifact should write");
    let before = fixture.authority_bytes();

    let first = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("read-only candidate union should collect");
    let second = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("repeat collection should collect");
    assert_eq!(first, second, "candidate ordering must be deterministic");
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "repeat enumeration must not mutate shared authority"
    );

    let [candidate] = first.candidates() else {
        panic!("one durable tenant-qualified candidate should exist");
    };
    assert_eq!(candidate.tenant_id(), &fixture.tenant_id);
    assert_eq!(
        candidate.attachment_id(),
        &default_network_attachment_id(&fixture.sandbox_id)
    );
    assert!(candidate.desired().is_some());
    assert!(candidate.provider().is_some());
    assert_eq!(candidate.allocator().len(), 2);
    assert_eq!(candidate.allocator()[1].reservation_claim(), &fixture.claim);
    assert_eq!(
        candidate
            .allocator()
            .iter()
            .map(OciExactAllocatorEvidence::source)
            .collect::<Vec<_>>(),
        [
            OciAllocatorEvidenceSource::DesiredAttachment,
            OciAllocatorEvidenceSource::ProviderAttempt,
        ]
    );
    assert!(candidate.allocator().iter().all(|evidence| {
        evidence.observation().is_ok_and(|observation| {
            observation.state() == NetworkAttachmentReservationState::Reserved
        })
    }));
    assert_eq!(candidate.artifacts().len(), 3);
    assert!(
        candidate
            .artifacts()
            .iter()
            .all(|artifact| { matches!(artifact.state(), OciArtifactObservationState::Present) })
    );
    assert!(first.unmatched_provider_evidence().is_empty());
    assert!(first.artifact_scan_unknowns().is_empty());
    assert_eq!(first.unmatched_artifacts().len(), 1);
    assert_eq!(first.unmatched_artifacts()[0].path(), unmatched_netns);
    assert_eq!(
        first.unmatched_artifacts()[0].kind(),
        OciArtifactKind::NetworkNamespace
    );
    assert!(
        first
            .candidates()
            .iter()
            .all(|candidate| candidate.attachment_id().as_str()
                != "artifact-name-is-not-an-attachment"),
        "an unmatched filename must never create canonical identity"
    );

    let reopened_attachments = LocalNetworkAttachmentAuthority::open(&fixture.network_root)
        .expect("attachment authority should reopen");
    let reopened_ipam = OciIpamAuthority::reconstruct_for_direct_test(&fixture.layout)
        .expect("IPAM authority should reopen");
    let reopened_allocator = SingleNodeSegmentAllocator::single_node_default(&fixture.network_root);
    let reopened = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &reopened_attachments,
        &reopened_ipam,
        &reopened_allocator,
    )
    .expect("fresh handles should reconstruct the same evidence");
    assert_eq!(reopened, first);
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "fresh reopen must preserve exact authority bytes"
    );
}

#[test]
fn different_artifact_realm_is_retained_separately_without_candidate_adoption() {
    let fixture = EvidenceFixture::new("realm-fence", AttachmentBackendKind::Container, false);
    let other_root = fixture._temp_dir.path().join("other-workloads");
    fs::create_dir_all(&other_root).expect("other workload root should exist");
    let before = fixture.authority_bytes();

    let report = collect_oci_orphan_evidence(
        &other_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("foreign realm should remain reportable");
    let [desired_only] = report.candidates() else {
        panic!("desired state should remain a current authority candidate");
    };
    assert!(desired_only.desired().is_some());
    assert!(desired_only.provider().is_none());
    let [unmatched] = report.unmatched_provider_evidence() else {
        panic!("foreign provider realm must be retained separately");
    };
    assert_eq!(unmatched.evidence().tenant_id(), &fixture.tenant_id);
    assert!(matches!(
        unmatched.realm(),
        OciProviderRealmObservation::DifferentRealm
    ));
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "realm rejection must not mutate durable authority"
    );
}

#[test]
fn exact_provider_attempt_identity_reopens_without_label_or_manifest_inference() {
    let fixture = EvidenceFixture::new(
        "provider-attempt-reopen",
        AttachmentBackendKind::Container,
        false,
    );
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("real provider owner should persist its prepared attempt");
    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("real provider owner should persist its ambiguous execution fence");
    let expected_attempt = setup_claim.operation_attempt().clone();
    let before = fixture.authority_bytes();

    let first = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("exact provider attempt should enumerate");
    let reopened_ipam = OciIpamAuthority::reconstruct_for_direct_test(&fixture.layout)
        .expect("provider authority should reopen");
    let reopened = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &reopened_ipam,
        &fixture.allocator,
    )
    .expect("reopened provider attempt should enumerate");
    assert_eq!(first, reopened);
    let [candidate] = reopened.candidates() else {
        panic!("one exact provider candidate should reopen");
    };
    assert!(matches!(
        candidate
            .provider()
            .expect("provider evidence should exist")
            .provider_operation(),
        NetavarkProviderOperation::Provisioning { operation_attempt }
            if operation_attempt == &expected_attempt
    ));
    assert!(
        candidate
            .artifacts()
            .iter()
            .all(|artifact| { matches!(artifact.state(), OciArtifactObservationState::Absent) }),
        "the exact durable provider attempt must not depend on manifest or filename inference"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "provider-attempt enumeration and reopen must be byte-stable"
    );
}

#[test]
fn provider_lifecycle_reauthenticates_the_artifact_realm_before_every_transition() {
    let fixture = EvidenceFixture::new(
        "lifecycle-realm-substitution",
        AttachmentBackendKind::Container,
        false,
    );
    let foreign_workload_root = fixture._temp_dir.path().join("foreign-workloads");
    fs::create_dir_all(&foreign_workload_root).expect("foreign workload root should exist");
    let foreign_layout = OciNetworkLayout::with_roots(
        &foreign_workload_root,
        &fixture.network_root,
        &fixture.tenant_id,
        &fixture.sandbox_id,
    );
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("canonical setup should prepare");

    let before_setup_execution = fixture.authority_bytes();
    let error = begin_netavark_setup_execution(
        &fixture.ipam,
        &foreign_layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect_err("setup execution must reject a substituted workload realm");
    assert!(
        error.to_string().contains("provider locator"),
        "setup execution should name its locator fence: {error}"
    );
    assert_eq!(fixture.authority_bytes(), before_setup_execution);

    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("canonical setup execution should cross its fence");
    let before_setup_completion = fixture.authority_bytes();
    let error = complete_netavark_setup(&fixture.ipam, &foreign_layout, &setup_claim)
        .expect_err("setup completion must reject a substituted workload realm");
    assert!(
        error.to_string().contains("provider locator"),
        "setup completion should name its locator fence: {error}"
    );
    assert_eq!(fixture.authority_bytes(), before_setup_completion);
    complete_netavark_setup(&fixture.ipam, &fixture.layout, &setup_claim)
        .expect("canonical setup completion should publish ready");

    let teardown_claim = match begin_netavark_teardown(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        None,
    )
    .expect("canonical teardown should prepare")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("a ready provider generation must require teardown"),
    };
    let before_teardown_execution = fixture.authority_bytes();
    let error = begin_netavark_teardown_execution(&fixture.ipam, &foreign_layout, &teardown_claim)
        .expect_err("teardown execution must reject a substituted workload realm");
    assert!(
        error.to_string().contains("provider locator"),
        "teardown execution should name its locator fence: {error}"
    );
    assert_eq!(fixture.authority_bytes(), before_teardown_execution);

    begin_netavark_teardown_execution(&fixture.ipam, &fixture.layout, &teardown_claim)
        .expect("canonical teardown execution should cross its fence");
    let before_detach_confirmation = fixture.authority_bytes();
    let error = confirm_netavark_provider_detached(&fixture.ipam, &foreign_layout, &teardown_claim)
        .expect_err("detach confirmation must reject a substituted workload realm");
    assert!(
        error.to_string().contains("provider locator"),
        "detach confirmation should name its locator fence: {error}"
    );
    assert_eq!(fixture.authority_bytes(), before_detach_confirmation);

    confirm_netavark_provider_detached(&fixture.ipam, &fixture.layout, &teardown_claim)
        .expect("canonical detach confirmation should publish absence");
    let before_teardown_completion = fixture.authority_bytes();
    let error = complete_netavark_teardown(&fixture.ipam, &foreign_layout, &teardown_claim)
        .expect_err("teardown completion must reject a substituted workload realm");
    assert!(
        error.to_string().contains("provider locator"),
        "teardown completion should name its locator fence: {error}"
    );
    assert_eq!(fixture.authority_bytes(), before_teardown_completion);
    complete_netavark_teardown(&fixture.ipam, &fixture.layout, &teardown_claim)
        .expect("canonical teardown completion should publish detached");
}

#[test]
fn provider_lifecycle_rejects_backend_substitution_before_preparing_an_effect() {
    let fixture = EvidenceFixture::new(
        "lifecycle-backend-substitution",
        AttachmentBackendKind::Container,
        false,
    );
    let mut substituted_config = fixture.config.clone();
    substituted_config.provider_kind = OciAttachmentProviderKind::Krun;
    let before = fixture.authority_bytes();

    let error = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &substituted_config,
        &fixture.sandbox_id,
    )
    .expect_err("setup preparation must reject a substituted provider kind");
    assert!(
        error.to_string().contains("provider locator"),
        "provider-kind substitution should name its locator fence: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "provider-kind rejection must not rewrite the attempt journal"
    );
}

#[test]
fn provider_attempts_are_bound_to_the_exact_claim_and_segment_generation() {
    for substitution in ["reservation-claim", "segment"] {
        let fixture = EvidenceFixture::new(
            &format!("attempt-generation-{substitution}"),
            AttachmentBackendKind::Container,
            false,
        );
        begin_netavark_setup(
            &fixture.ipam,
            &fixture.layout,
            &fixture.config,
            &fixture.sandbox_id,
        )
        .expect("real provider owner should persist a generation-bound attempt");
        let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
        store
            .transaction(
                &NetworkStatePartition::TenantIpam(fixture.tenant_id.clone()),
                |state: &mut IpamState| {
                    let allocation = state
                        .allocations
                        .values_mut()
                        .next()
                        .expect("fixture allocation should exist");
                    match substitution {
                        "reservation-claim" => {
                            allocation.reservation_claim =
                                reservation_claim("foreign-attempt-generation");
                        }
                        "segment" => {
                            allocation.segment_id =
                                NetworkSegmentId::generate().as_str().to_owned();
                        }
                        _ => unreachable!("the test table is closed"),
                    }
                    Ok::<_, SandboxError>(())
                },
            )
            .expect("checksum-valid generation substitution should install");
        let before = fixture.authority_bytes();

        let error = collect_oci_orphan_evidence(
            &fixture.workload_root,
            &fixture.attachments,
            &fixture.ipam,
            &fixture.allocator,
        )
        .expect_err("a prior-generation provider attempt must fail closed");
        assert!(
            error.to_string().contains("generation binding"),
            "{substitution} substitution should name its attempt-generation fence: {error}"
        );
        assert_eq!(
            fixture.authority_bytes(),
            before,
            "{substitution} rejection must preserve exact authority bytes"
        );
    }
}

#[test]
fn provider_attempt_substitution_is_rejected_without_authority_mutation() {
    const PROVIDER: &str = "nimbus-sandbox.oci.netavark-operation";
    const ATTEMPT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    let fixture = EvidenceFixture::new(
        "provider-attempt-substitution",
        AttachmentBackendKind::Container,
        false,
    );
    let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
    let foreign_attachment =
        NetworkAttachmentId::for_workload_attachment("foreign-sandbox", "default");
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("valid setup attempt should prepare");
    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("valid setup attempt should cross its effect fence");
    complete_netavark_setup(&fixture.ipam, &fixture.layout, &setup_claim)
        .expect("valid setup attempt should complete");
    let teardown_claim = match begin_netavark_teardown(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        None,
    )
    .expect("valid teardown attempt should prepare")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("ready setup should produce a provider teardown"),
    };
    let valid_setup = setup_claim.operation_attempt().clone();
    let valid_teardown = teardown_claim.operation_attempt().clone();
    let setup_parts = valid_setup
        .expose_to_provider()
        .split(':')
        .collect::<Vec<_>>();
    assert_eq!(setup_parts.len(), 6);
    let generation_digest = setup_parts[4];
    let cases = [
        (
            "foreign-provider",
            NetavarkProviderOperation::SetupPrepared {
                operation_attempt: netavark_attempt(
                    "nimbus-sandbox.foreign-provider",
                    "v1",
                    "setup",
                    &fixture.tenant_id,
                    &attachment_id,
                    generation_digest,
                    ATTEMPT,
                ),
            },
        ),
        (
            "foreign-tenant",
            NetavarkProviderOperation::Provisioning {
                operation_attempt: netavark_attempt(
                    PROVIDER,
                    "v1",
                    "setup",
                    &TenantId::new("foreign-tenant").expect("tenant should validate"),
                    &attachment_id,
                    generation_digest,
                    ATTEMPT,
                ),
            },
        ),
        (
            "foreign-attachment",
            NetavarkProviderOperation::Ready {
                setup_attempt: netavark_attempt(
                    PROVIDER,
                    "v1",
                    "setup",
                    &fixture.tenant_id,
                    &foreign_attachment,
                    generation_digest,
                    ATTEMPT,
                ),
            },
        ),
        (
            "wrong-setup-action",
            NetavarkProviderOperation::Provisioning {
                operation_attempt: valid_teardown.clone(),
            },
        ),
        (
            "malformed-attempt-id",
            NetavarkProviderOperation::SetupPrepared {
                operation_attempt: netavark_attempt(
                    PROVIDER,
                    "v1",
                    "setup",
                    &fixture.tenant_id,
                    &attachment_id,
                    generation_digest,
                    "not-a-ulid",
                ),
            },
        ),
        (
            "generation-digest-substitution",
            NetavarkProviderOperation::SetupPrepared {
                operation_attempt: netavark_attempt(
                    PROVIDER,
                    "v1",
                    "setup",
                    &fixture.tenant_id,
                    &attachment_id,
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    ATTEMPT,
                ),
            },
        ),
        (
            "teardown-setup-attempt-substitution",
            NetavarkProviderOperation::TeardownPrepared {
                setup_attempt: valid_teardown.clone(),
                operation_attempt: valid_teardown.clone(),
            },
        ),
        (
            "teardown-operation-attempt-substitution",
            NetavarkProviderOperation::Deleting {
                setup_attempt: valid_setup.clone(),
                operation_attempt: valid_setup,
            },
        ),
    ];
    let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
    let partition = NetworkStatePartition::TenantIpam(fixture.tenant_id.clone());
    for (label, substituted_operation) in cases {
        store
            .transaction(&partition, |state: &mut IpamState| {
                state
                    .allocations
                    .values_mut()
                    .next()
                    .expect("fixture allocation should exist")
                    .provider_operation = substituted_operation.clone();
                Ok::<_, SandboxError>(())
            })
            .unwrap_or_else(|error| panic!("{label} fixture should persist: {error}"));
        let before = fixture.authority_bytes();

        let error = collect_oci_orphan_evidence(
            &fixture.workload_root,
            &fixture.attachments,
            &fixture.ipam,
            &fixture.allocator,
        )
        .expect_err("provider-attempt substitution must fail closed");
        assert!(
            error.to_string().contains("OCI Netavark"),
            "{label} should produce a named provider-attempt error: {error}"
        );
        assert_eq!(
            fixture.authority_bytes(),
            before,
            "{label} rejection must preserve exact authority bytes"
        );
    }
}

#[test]
fn conflicting_claim_and_non_file_artifact_remain_typed_unknown_evidence() {
    let fixture = EvidenceFixture::new("typed-unknown", AttachmentBackendKind::Container, true);
    fs::create_dir_all(&fixture.layout.netns_path)
        .expect("non-file netns observation should exist");
    let before = fixture.authority_bytes();

    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("claim and artifact conflicts are evidence, not collector failure");
    let [candidate] = report.candidates() else {
        panic!("one candidate should remain");
    };
    assert!(matches!(
        candidate.allocator()[0].observation(),
        Err(unknown)
            if unknown.operation() == "inspect exact allocator reservation"
                && unknown.error_kind() == "Domain"
                && unknown.message().contains("different launch reservation coordinator")
    ));
    assert!(candidate.allocator()[1].observation().is_ok());
    assert!(candidate.artifacts().iter().any(|artifact| {
        artifact.kind() == OciArtifactKind::NetworkNamespace
            && matches!(
                artifact.state(),
                OciArtifactObservationState::Unknown(unknown)
                    if unknown.error_kind() == "UnexpectedFileType"
                        && unknown.path() == Some(fixture.layout.netns_path.as_path())
            )
    }));
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "unknown evidence must never trigger authority mutation"
    );
}

#[test]
fn both_provider_kinds_persist_as_exact_typed_evidence() {
    for (label, backend, expected) in [
        (
            "container-route",
            AttachmentBackendKind::Container,
            OciAttachmentProviderKind::Container,
        ),
        (
            "krun-route",
            AttachmentBackendKind::Krun,
            OciAttachmentProviderKind::Krun,
        ),
    ] {
        let fixture = EvidenceFixture::new(label, backend, false);
        let evidence = fixture
            .ipam
            .get_attachment_provider_evidence(
                &fixture.tenant_id,
                &default_network_attachment_id(&fixture.sandbox_id),
            )
            .expect("provider evidence should inspect")
            .expect("provider evidence should exist");
        assert_eq!(evidence.provider_kind(), expected);
        assert_eq!(evidence.lifecycle(), OciIpamEvidenceLifecycle::Live);
        assert!(matches!(
            evidence.provider_operation(),
            NetavarkProviderOperation::Reserved
        ));
        assert_eq!(evidence.segment_id().as_str(), fixture.config.segment_id);
        assert_eq!(evidence.reservation_claim(), &fixture.claim);
        assert!(
            evidence
                .authenticates_workload_root(&fixture.workload_root)
                .expect("configured root should authenticate")
        );
        assert!(
            evidence
                .artifact_realm_id()
                .as_str()
                .starts_with("oci-artifact-realm-v2-sha256:")
        );
    }
}

#[test]
fn malformed_tenant_ipam_key_fails_closed_without_collector_mutation() {
    let fixture = EvidenceFixture::new("malformed-key", AttachmentBackendKind::Container, false);
    let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
    let partition = NetworkStatePartition::TenantIpam(fixture.tenant_id.clone());
    store
        .transaction(&partition, |state: &mut IpamState| {
            let allocation = state
                .allocations
                .values()
                .next()
                .expect("fixture allocation should exist")
                .clone();
            state
                .allocations
                .insert("not-a-network-attachment-id".to_owned(), allocation);
            Ok::<_, SandboxError>(())
        })
        .expect("checksum-valid malformed state should install");
    let before = fixture.authority_bytes();

    let error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("malformed durable key must fail closed");
    assert!(
        error.to_string().contains("invalid attachment key"),
        "collector should name the exact schema violation: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "rejected malformed evidence must remain byte-for-byte unchanged"
    );
}

#[test]
fn duplicate_live_and_terminal_provider_authority_fails_closed_without_mutation() {
    let fixture = EvidenceFixture::new(
        "duplicate-provider-authority",
        AttachmentBackendKind::Container,
        false,
    );
    let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
    let partition = NetworkStatePartition::TenantIpam(fixture.tenant_id.clone());
    store
        .transaction(&partition, |state: &mut IpamState| {
            let (attachment_key, allocation) = {
                let (attachment_key, allocation) = state
                    .allocations
                    .first_key_value()
                    .expect("fixture allocation should exist");
                (attachment_key.clone(), allocation.clone())
            };
            state
                .released_allocations
                .insert(attachment_key, allocation);
            Ok::<_, SandboxError>(())
        })
        .expect("checksum-valid duplicate authority fixture should install");
    let before = fixture.authority_bytes();

    let error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("live and terminal authority for one attachment must fail closed");
    assert!(
        error
            .to_string()
            .contains("both live and terminal IPAM authority"),
        "duplicate provider authority should name the conflicting lifecycle: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "duplicate provider authority rejection must not rewrite either record"
    );
}

#[test]
fn terminal_provider_evidence_requires_a_no_effect_or_detached_phase() {
    let fixture = EvidenceFixture::new(
        "terminal-provider-phase",
        AttachmentBackendKind::Container,
        false,
    );
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("setup should prepare");
    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("setup should enter an ambiguous provider phase");
    let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
    store
        .transaction(
            &NetworkStatePartition::TenantIpam(fixture.tenant_id.clone()),
            |state: &mut IpamState| {
                let (attachment_key, allocation) = state
                    .allocations
                    .pop_first()
                    .expect("fixture allocation should exist");
                state
                    .released_allocations
                    .insert(attachment_key, allocation);
                Ok::<_, SandboxError>(())
            },
        )
        .expect("checksum-valid terminal phase substitution should install");
    let before = fixture.authority_bytes();

    let error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("terminal evidence must reject a possibly live provider effect");
    assert!(
        error.to_string().contains("terminal IPAM authority")
            && error.to_string().contains("provisioning"),
        "terminal phase rejection should name the authority and live phase: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "terminal phase validation must remain read-only"
    );
}

#[test]
fn tenant_sandbox_and_attachment_substitution_fail_closed_without_mutation() {
    let fixture = EvidenceFixture::new(
        "identity-substitution",
        AttachmentBackendKind::Container,
        false,
    );
    let store = LocalNetworkStateStore::open(&fixture.network_root).expect("store should open");
    let source_partition = NetworkStatePartition::TenantIpam(fixture.tenant_id.clone());
    let source: IpamState = store
        .read(&source_partition)
        .expect("source partition should read")
        .expect("source partition should exist");
    let source_allocation = source
        .allocations
        .values()
        .next()
        .expect("fixture allocation should exist")
        .clone();

    let foreign_tenant =
        TenantId::new("nnc52b-foreign-tenant").expect("foreign tenant should validate");
    store
        .transaction(
            &NetworkStatePartition::TenantIpam(foreign_tenant.clone()),
            |state: &mut IpamState| {
                state.allocations.insert(
                    default_network_attachment_id(&fixture.sandbox_id)
                        .as_str()
                        .to_owned(),
                    source_allocation.clone(),
                );
                Ok::<_, SandboxError>(())
            },
        )
        .expect("checksum-valid foreign partition fixture should install");
    let before_tenant_rejection = fixture.authority_bytes();
    let tenant_error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("a locator copied into another tenant partition must fail closed");
    assert!(
        tenant_error
            .to_string()
            .contains("does not match provider locator tenant"),
        "tenant substitution should name its exact mismatch: {tenant_error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before_tenant_rejection,
        "tenant substitution rejection must remain read-only"
    );

    store
        .transaction(
            &NetworkStatePartition::TenantIpam(foreign_tenant),
            |state: &mut IpamState| {
                state.allocations.clear();
                Ok::<_, SandboxError>(())
            },
        )
        .expect("foreign tenant fixture should be removed before the next substitution");
    store
        .transaction(&source_partition, |state: &mut IpamState| {
            let allocation = state
                .allocations
                .values_mut()
                .next()
                .expect("fixture allocation should remain");
            let mut locator = serde_json::to_value(&allocation.provider_locator)
                .expect("locator should serialize");
            locator["sandbox_id"] = serde_json::json!("substituted-sandbox");
            allocation.provider_locator = serde_json::from_value(locator)
                .expect("schema-valid substituted locator should parse");
            Ok::<_, SandboxError>(())
        })
        .expect("checksum-valid sandbox substitution fixture should install");
    let before_sandbox_rejection = fixture.authority_bytes();
    let error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("locator sandbox substitution must fail");
    assert!(
        error.to_string().contains("does not match locator sandbox"),
        "sandbox substitution should name its attachment mismatch: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before_sandbox_rejection,
        "sandbox substitution rejection must not mutate authority"
    );

    store
        .transaction(&source_partition, |state: &mut IpamState| {
            state.allocations.clear();
            let attachment_key = default_network_attachment_id(&fixture.sandbox_id);
            let substituted_attachment =
                NetworkAttachmentId::for_workload_attachment("foreign-sandbox", "default");
            assert_ne!(substituted_attachment, attachment_key);
            state.allocations.insert(
                substituted_attachment.as_str().to_owned(),
                source_allocation.clone(),
            );
            Ok::<_, SandboxError>(())
        })
        .expect("checksum-valid attachment substitution fixture should install");
    let before_attachment_rejection = fixture.authority_bytes();
    let error = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect_err("attachment key substitution must fail");
    assert!(
        error.to_string().contains("does not match locator sandbox"),
        "attachment substitution should name its locator mismatch: {error}"
    );
    assert_eq!(
        fixture.authority_bytes(),
        before_attachment_rejection,
        "attachment substitution rejection must not mutate authority"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_artifact_owner_is_retained_as_unknown_without_lossy_identity() {
    use std::os::unix::fs::symlink;

    let fixture = EvidenceFixture::new("symlink-unknown", AttachmentBackendKind::Container, false);
    let foreign_tenant_path = fixture
        .workload_root
        .join("tenants")
        .join("not-a-canonical-tenant-source");
    symlink(
        fixture._temp_dir.path().join("missing-symlink-target"),
        &foreign_tenant_path,
    )
    .expect("untrusted tenant symlink should install");
    let before = fixture.authority_bytes();

    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("symlink observation should be retained, not flattened");
    assert!(report.artifact_scan_unknowns().iter().any(|unknown| {
        unknown.operation() == "enumerate tenant artifact roots"
            && unknown.path() == Some(foreign_tenant_path.as_path())
            && unknown.error_kind() == "UnexpectedFileType"
    }));
    assert!(
        report
            .candidates()
            .iter()
            .all(|candidate| candidate.tenant_id().as_str() != "not-a-canonical-tenant-source"),
        "an artifact directory name must not create tenant authority"
    );
    assert_eq!(fixture.authority_bytes(), before);
}

#[cfg(unix)]
#[test]
fn symlinked_intermediate_artifact_root_cannot_escape_the_authenticated_realm() {
    use std::os::unix::fs::symlink;

    let fixture = EvidenceFixture::new(
        "symlinked-intermediate-root",
        AttachmentBackendKind::Container,
        false,
    );
    let scan_root = fixture._temp_dir.path().join("scan-realm");
    let outside_tenants = fixture._temp_dir.path().join("outside-tenants");
    let escaped_artifact = outside_tenants
        .join("outside-tenant")
        .join("networks")
        .join("netns")
        .join("escaped-netns");
    fs::create_dir_all(
        escaped_artifact
            .parent()
            .expect("escaped artifact should have a parent"),
    )
    .expect("outside artifact hierarchy should exist");
    fs::write(&escaped_artifact, b"outside authenticated artifact realm")
        .expect("outside artifact should exist");
    fs::create_dir_all(&scan_root).expect("scan realm should exist");
    let symlinked_tenants = scan_root.join("tenants");
    symlink(&outside_tenants, &symlinked_tenants)
        .expect("intermediate tenant symlink should install");
    let before = fixture.authority_bytes();

    let report = collect_oci_orphan_evidence(
        &scan_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("an escape attempt should remain typed unknown evidence");
    assert!(
        report
            .unmatched_artifacts()
            .iter()
            .all(|artifact| artifact.path() != escaped_artifact),
        "the collector must never enumerate an artifact outside the pinned realm"
    );
    assert!(report.artifact_scan_unknowns().iter().any(|unknown| {
        unknown.operation() == "enumerate tenant artifact roots"
            && unknown.path() == Some(symlinked_tenants.as_path())
    }));
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "symlink escape evidence must never trigger authority mutation"
    );
}

#[cfg(unix)]
#[test]
fn retargeted_workload_root_cannot_join_provider_evidence_to_a_different_pinned_realm() {
    use std::os::unix::fs::symlink;

    let pinned_owner =
        EvidenceFixture::new("retargeted-root", AttachmentBackendKind::Container, false);
    let substituted_owner =
        EvidenceFixture::new("retargeted-root", AttachmentBackendKind::Container, false);
    let alias_parent = TempDir::new().expect("alias parent should exist");
    let injected_root = alias_parent.path().join("injected-workload-root");
    symlink(&pinned_owner.workload_root, &injected_root)
        .expect("injected root should initially select the pinned owner");
    let pinned = PinnedArtifactRealm::open(&injected_root);

    fs::remove_file(&injected_root).expect("original injected-root link should remove");
    symlink(&substituted_owner.workload_root, &injected_root)
        .expect("injected root should retarget to substituted owner");
    let substituted_provider = substituted_owner
        .ipam
        .get_attachment_provider_evidence(
            &substituted_owner.tenant_id,
            &default_network_attachment_id(&substituted_owner.sandbox_id),
        )
        .expect("substituted provider evidence should inspect")
        .expect("substituted provider evidence should exist");

    assert!(
        !pinned
            .authenticates_provider(&substituted_provider)
            .expect("pinned realm comparison should be deterministic"),
        "provider evidence from the retargeted path must not authenticate against the directory \
         capability that was opened before retargeting"
    );
}

#[test]
fn non_not_found_artifact_enumeration_error_is_retained_as_typed_unknown() {
    let fixture = EvidenceFixture::new(
        "scan-error-unknown",
        AttachmentBackendKind::Container,
        false,
    );
    fs::remove_dir(&fixture.layout.netns_root).expect("empty netns directory should be removable");
    fs::write(
        &fixture.layout.netns_root,
        b"not a directory; read_dir must return a typed error",
    )
    .expect("netns root file should install");
    let before = fixture.authority_bytes();

    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("non-NotFound scan errors should remain evidence");
    assert!(report.artifact_scan_unknowns().iter().any(|unknown| {
        unknown.operation() == "enumerate persistent network namespaces"
            && unknown.path() == Some(fixture.layout.netns_root.as_path())
            && unknown.error_kind() == "UnexpectedFileType"
            && unknown.message().contains("non-symlink directory")
    }));
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "typed scan failures must not mutate authority"
    );
}

#[test]
fn genuinely_new_process_reopens_the_same_durable_candidate_without_handoff_memory() {
    let fixture = EvidenceFixture::new("fresh-process", AttachmentBackendKind::Krun, false);
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("real provider owner should prepare an exact setup attempt");
    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("real provider owner should persist the ambiguous setup fence");
    fixture.publish_exact_artifacts();
    let before = fixture.authority_bytes();
    let output =
        Command::new(std::env::current_exe().expect("current test executable should resolve"))
            .args([
                "--ignored",
                "--exact",
                "backends::oci::network::orphan_evidence::tests::nnc5_2b_fresh_process_child",
                "--nocapture",
            ])
            .env("NIMBUS_NNC52B_WORKLOAD_ROOT", &fixture.workload_root)
            .env("NIMBUS_NNC52B_NETWORK_ROOT", &fixture.network_root)
            .env("NIMBUS_NNC52B_TENANT", fixture.tenant_id.as_str())
            .env("NIMBUS_NNC52B_SANDBOX", fixture.sandbox_id.as_str())
            .output()
            .expect("fresh evidence process should start");
    assert!(
        output.status.success(),
        "fresh evidence process failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(
            "NNC5.2b fresh process: candidates=1 allocator=2 artifacts=3 provider=provisioning"
        ),
        "child must report the exact reconstructed evidence shape:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fixture.authority_bytes(),
        before,
        "fresh process enumeration must not rewrite authority"
    );
}

#[test]
#[ignore = "subprocess entry point; the parent test supplies exact durable roots"]
fn nnc5_2b_fresh_process_child() {
    let workload_root = PathBuf::from(
        std::env::var_os("NIMBUS_NNC52B_WORKLOAD_ROOT").expect("parent must provide workload root"),
    );
    let network_root = PathBuf::from(
        std::env::var_os("NIMBUS_NNC52B_NETWORK_ROOT").expect("parent must provide network root"),
    );
    let tenant_id =
        TenantId::new(std::env::var("NIMBUS_NNC52B_TENANT").expect("parent must provide tenant"))
            .expect("parent tenant should validate");
    let sandbox_id = SandboxId::new(
        std::env::var("NIMBUS_NNC52B_SANDBOX").expect("parent must provide sandbox"),
    );
    let layout =
        OciNetworkLayout::with_roots(&workload_root, &network_root, &tenant_id, &sandbox_id);
    let attachments = LocalNetworkAttachmentAuthority::open(&network_root)
        .expect("child attachment authority should reopen");
    let ipam = OciIpamAuthority::reconstruct_for_direct_test(&layout)
        .expect("child IPAM authority should reopen");
    let allocator = SingleNodeSegmentAllocator::single_node_default(&network_root);
    let report = collect_oci_orphan_evidence(&workload_root, &attachments, &ipam, &allocator)
        .expect("child should reconstruct candidate evidence");
    let [candidate] = report.candidates() else {
        panic!("child should reconstruct exactly one candidate");
    };
    assert_eq!(candidate.tenant_id(), &tenant_id);
    assert_eq!(
        candidate.attachment_id(),
        &default_network_attachment_id(&sandbox_id)
    );
    assert!(candidate.desired().is_some());
    assert_eq!(
        candidate
            .provider()
            .expect("provider evidence should reopen")
            .provider_kind(),
        OciAttachmentProviderKind::Krun
    );
    assert!(matches!(
        candidate
            .provider()
            .expect("provider evidence should reopen")
            .provider_operation(),
        NetavarkProviderOperation::Provisioning { .. }
    ));
    assert_eq!(candidate.allocator().len(), 2);
    assert_eq!(candidate.artifacts().len(), 3);
    assert!(
        candidate
            .artifacts()
            .iter()
            .all(|artifact| { matches!(artifact.state(), OciArtifactObservationState::Present) })
    );
    println!(
        "NNC5.2b fresh process: candidates={} allocator={} artifacts={} provider=provisioning",
        report.candidates().len(),
        candidate.allocator().len(),
        candidate.artifacts().len()
    );
}
