use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, LocalNetworkStateStore,
    NetworkAttachmentReservationObservation, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId,
    NetworkProviderHandle, NetworkResourceGeneration, NetworkResourcePhase,
    NetworkSegmentAllocator, NetworkStatePartition, NetworkStateTransition,
    NetworkTransitionEvidence,
};

use super::*;
use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_requirements,
};
use crate::backends::oci::network::attachment_lifecycle::{
    AttachmentBackendKind, oci_attachment_plan, oci_attachment_provider_handle,
};
use crate::backends::oci::network::default_network_attachment_id;
use crate::backends::oci::network::dto::{IpamState, NetavarkProviderOperation};
use crate::backends::oci::network::ipam::{
    begin_netavark_setup, begin_netavark_setup_execution, complete_netavark_setup,
};
use crate::backends::oci::network::orphan_evidence::test_support::EvidenceFixture;
use crate::backends::oci::network::orphan_evidence::{
    OciArtifactKind, OciArtifactObservationState, OciEvidenceUnknown, OciOrphanEvidenceCandidate,
    OciOrphanEvidenceReport, OciProviderRealmObservation, collect_oci_orphan_evidence,
};
use crate::error::SandboxError;

fn ready_report(
    label: &str,
    desired_claim_substitution: bool,
) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    let fixture = EvidenceFixture::new(
        label,
        AttachmentBackendKind::Container,
        desired_claim_substitution,
    );
    report_with_ready_provider(fixture, NetworkResourcePhase::Ready)
}

fn report_with_ready_provider(
    fixture: EvidenceFixture,
    desired_phase: NetworkResourcePhase,
) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    report_with_ready_provider_handle(fixture, desired_phase, true)
}

fn report_with_ready_provider_handle(
    fixture: EvidenceFixture,
    desired_phase: NetworkResourcePhase,
    record_stable_handle: bool,
) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    let provider_handle = record_stable_handle.then(|| {
        let desired = fixture
            .attachments
            .get(
                &fixture.tenant_id,
                &default_network_attachment_id(&fixture.sandbox_id),
            )
            .expect("desired authority should inspect")
            .expect("desired record should exist");
        let backend = if desired.selected_provider_id()
            == &selected_provider_id(OciAttachmentProviderKind::Container)
        {
            AttachmentBackendKind::Container
        } else if desired.selected_provider_id()
            == &selected_provider_id(OciAttachmentProviderKind::Krun)
        {
            AttachmentBackendKind::Krun
        } else {
            panic!("fixture selected an unsupported OCI attachment provider");
        };
        oci_attachment_provider_handle(&fixture.tenant_id, &fixture.sandbox_id, backend)
            .expect("canonical provider handle should validate")
    });
    report_with_ready_provider_explicit_handle(fixture, desired_phase, provider_handle)
}

fn report_with_ready_provider_explicit_handle(
    fixture: EvidenceFixture,
    desired_phase: NetworkResourcePhase,
    provider_handle: Option<NetworkProviderHandle>,
) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    fixture
        .allocator
        .adopt_reserved_attachment(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
            &fixture.claim,
        )
        .expect("exact reservation should adopt before provider effects");
    transition_desired_to(&fixture, desired_phase, provider_handle);
    let (_, setup_claim) = begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("provider setup attempt should prepare");
    begin_netavark_setup_execution(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
        &setup_claim,
    )
    .expect("provider setup should cross its exact pre-effect fence");
    complete_netavark_setup(&fixture.ipam, &fixture.layout, &setup_claim)
        .expect("provider setup should become ready");
    fixture.publish_exact_artifacts();

    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("exact evidence should collect");
    (fixture, report)
}

fn transition_desired_to(
    fixture: &EvidenceFixture,
    target: NetworkResourcePhase,
    provider_handle: Option<NetworkProviderHandle>,
) {
    let mut desired = fixture
        .attachments
        .get(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
        )
        .expect("desired authority should inspect")
        .expect("desired record should exist");
    if target == NetworkResourcePhase::Reserved {
        return;
    }
    for phase in [
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
    ] {
        let (_, next) = fixture
            .attachments
            .apply_transition(
                &fixture.tenant_id,
                &NetworkStateTransition::new(
                    desired.resource().version().clone(),
                    phase,
                    NetworkTransitionEvidence::Progress,
                ),
            )
            .unwrap_or_else(|error| panic!("desired attachment should enter {phase:?}: {error}"));
        desired = next;
        if phase == NetworkResourcePhase::Provisioning
            && let Some(provider_handle) = provider_handle.as_ref()
        {
            let (_, with_handle) = fixture
                .attachments
                .record_provider_handle(
                    &fixture.tenant_id,
                    desired.resource().version(),
                    provider_handle.clone(),
                )
                .expect("provisioning desired state should retain its stable provider handle");
            desired = with_handle;
        }
        if phase == target {
            return;
        }
    }
    panic!("test helper cannot drive desired attachment to {target:?}");
}

fn report_with_prepared_provider(label: &str) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    let fixture = EvidenceFixture::new(label, AttachmentBackendKind::Container, false);
    fixture
        .allocator
        .adopt_reserved_attachment(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
            &fixture.claim,
        )
        .expect("exact reservation should adopt");
    transition_desired_to(&fixture, NetworkResourcePhase::Provisioning, None);
    begin_netavark_setup(
        &fixture.ipam,
        &fixture.layout,
        &fixture.config,
        &fixture.sandbox_id,
    )
    .expect("provider setup should remain durably prepared");
    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("prepared evidence should collect");
    (fixture, report)
}

fn report_with_terminal_provider(label: &str) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    let fixture = EvidenceFixture::new(label, AttachmentBackendKind::Container, false);
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
        .expect("no-effect provider evidence should become terminal");
    let report = collect_oci_orphan_evidence(
        &fixture.workload_root,
        &fixture.attachments,
        &fixture.ipam,
        &fixture.allocator,
    )
    .expect("terminal evidence should collect");
    (fixture, report)
}

fn report_with_substituted_provider_handle(
    label: &str,
    desired_phase: NetworkResourcePhase,
) -> (EvidenceFixture, OciOrphanEvidenceReport) {
    let fixture = EvidenceFixture::new(label, AttachmentBackendKind::Container, false);
    let provider_handle = NetworkProviderHandle::new(
        selected_provider_id(fixture.config.provider_kind()),
        format!(
            "attachment:substituted:{}",
            default_network_attachment_id(&fixture.sandbox_id)
        ),
    )
    .expect("same-provider substituted handle should validate");
    report_with_ready_provider_explicit_handle(fixture, desired_phase, Some(provider_handle))
}

fn only_candidate(report: &mut OciOrphanEvidenceReport) -> &mut OciOrphanEvidenceCandidate {
    let [candidate] = report.candidates.as_mut_slice() else {
        panic!("matrix fixture should contain one candidate");
    };
    candidate
}

fn only_disposition(report: &OciOrphanClassificationReport<'_>) -> OciOrphanDisposition {
    let mut dispositions = report
        .candidate_classifications()
        .iter()
        .map(OciEvidenceClassification::disposition)
        .collect::<Vec<_>>();
    dispositions.extend(
        report
            .unmatched_provider_classifications()
            .iter()
            .map(OciEvidenceClassification::disposition),
    );
    dispositions.extend(
        report
            .unmatched_artifact_classifications()
            .iter()
            .map(OciEvidenceClassification::disposition),
    );
    dispositions.extend(
        report
            .artifact_scan_unknown_classifications()
            .iter()
            .map(OciEvidenceClassification::disposition),
    );
    let [disposition] = dispositions.as_slice() else {
        panic!("matrix row should classify exactly one subject: {report:?}");
    };
    *disposition
}

fn candidate_disposition(report: &OciOrphanEvidenceReport) -> OciOrphanDisposition {
    let classifications = classify_oci_orphan_evidence(report);
    let [classification] = classifications.candidate_classifications() else {
        panic!("fixture should classify exactly one candidate: {classifications:?}");
    };
    classification.disposition()
}

fn quarantine_reason(disposition: OciOrphanDisposition) -> OciOrphanQuarantineReason {
    match disposition {
        OciOrphanDisposition::Quarantine(reason) => reason,
        OciOrphanDisposition::Adopt => panic!("unsafe fixture unexpectedly adopted"),
    }
}

fn dispositions<Evidence>(
    classifications: &[OciEvidenceClassification<'_, Evidence>],
) -> Vec<OciOrphanDisposition> {
    classifications
        .iter()
        .map(OciEvidenceClassification::disposition)
        .collect()
}

fn reserve_substituted_desired(
    fixture: &EvidenceFixture,
    suffix: &str,
    plan: &NetworkPlan,
) -> DurableNetworkAttachmentState {
    let authority = LocalNetworkAttachmentAuthority::open(
        fixture
            ._temp_dir
            .path()
            .join(format!("substituted-desired-{suffix}")),
    )
    .expect("substituted desired authority should open");
    let current = fixture
        .attachments
        .get(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
        )
        .expect("current desired authority should inspect")
        .expect("current desired record should exist");
    authority
        .reserve(
            &fixture.tenant_id,
            current.selected_provider_id().clone(),
            plan,
            current
                .attachment_id()
                .expect("current attachment identity should validate")
                .clone(),
            current.association().clone(),
        )
        .expect("substituted desired record should be internally valid")
}

#[test]
fn nnc5_2c_pure_orphan_classifier_covers_complete_evidence_matrix() {
    let mut cases = BTreeMap::new();

    let (_fixture, exact) = ready_report("classifier-exact", false);
    cases.insert(
        "hold-desired-effect",
        (
            OciOrphanDisposition::Adopt,
            classify_oci_orphan_evidence(&exact),
        ),
    );

    let (_fixture, mut no_desired) = ready_report("classifier-no-desired", false);
    only_candidate(&mut no_desired).desired = None;
    cases.insert(
        "hold-no-desired-effect",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::DesiredAttachmentMissing),
            classify_oci_orphan_evidence(&no_desired),
        ),
    );

    let (_fixture, mut no_netns) = ready_report("classifier-no-netns", false);
    let netns = only_candidate(&mut no_netns)
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.kind == OciArtifactKind::NetworkNamespace)
        .expect("netns observation should exist");
    netns.state = OciArtifactObservationState::Absent;
    cases.insert(
        "hold-no-netns",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::NetworkNamespaceMissing),
            classify_oci_orphan_evidence(&no_netns),
        ),
    );

    let (_fixture, mut no_hold) = ready_report("classifier-no-hold", false);
    for allocator in &mut only_candidate(&mut no_hold).allocator {
        allocator.observation = Ok(NetworkAttachmentReservationObservation::absent());
    }
    cases.insert(
        "effect-no-hold",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::AllocatorHoldMissing),
            classify_oci_orphan_evidence(&no_hold),
        ),
    );

    let (_fixture, mut manifest_only) = ready_report("classifier-manifest-only", false);
    let candidate = manifest_only
        .candidates
        .pop()
        .expect("manifest fixture should contain one candidate");
    let manifest = candidate
        .artifacts
        .into_iter()
        .find(|artifact| artifact.kind == OciArtifactKind::Manifest)
        .expect("manifest observation should exist");
    manifest_only.unmatched_artifacts.push(manifest);
    cases.insert(
        "manifest-no-hold",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::UnmatchedArtifact),
            classify_oci_orphan_evidence(&manifest_only),
        ),
    );

    let (_fixture, mut no_manifest) = ready_report("classifier-no-manifest", false);
    let manifest = only_candidate(&mut no_manifest)
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.kind == OciArtifactKind::Manifest)
        .expect("manifest observation should exist");
    manifest.state = OciArtifactObservationState::Absent;
    cases.insert(
        "hold-netns-no-manifest",
        (
            OciOrphanDisposition::Adopt,
            classify_oci_orphan_evidence(&no_manifest),
        ),
    );

    let (_fixture, stale) = ready_report("classifier-stale-generation", true);
    cases.insert(
        "stale-generation",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence),
            classify_oci_orphan_evidence(&stale),
        ),
    );

    let (_fixture, mut unknown) = ready_report("classifier-unknown", false);
    let netns = only_candidate(&mut unknown)
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.kind == OciArtifactKind::NetworkNamespace)
        .expect("netns observation should exist");
    netns.state = OciArtifactObservationState::Unknown(OciEvidenceUnknown::domain(
        "inspect exact artifact",
        "forced matrix inspection failure",
    ));
    cases.insert(
        "unknown-inspection",
        (
            OciOrphanDisposition::Quarantine(OciOrphanQuarantineReason::UnknownInspection),
            classify_oci_orphan_evidence(&unknown),
        ),
    );

    assert_eq!(cases.len(), 8, "every required evidence row must run");
    let mismatches = cases
        .iter()
        .filter_map(|(name, (expected, report))| {
            let observed = only_disposition(report);
            (*expected != observed).then_some((*name, (*expected, observed)))
        })
        .collect::<BTreeMap<_, _>>();
    assert!(
        mismatches.is_empty(),
        "NNC5.2c must classify every immutable evidence row exactly; mismatches: \
         {mismatches:#?}; complete reports: {cases:#?}"
    );
}

#[test]
fn exact_ready_fixture_uses_the_real_closed_provider_and_allocator_states() {
    let (_fixture, report) = ready_report("classifier-preconditions", false);
    let [candidate] = report.candidates() else {
        panic!("one exact candidate should exist");
    };
    assert!(matches!(
        candidate
            .provider()
            .expect("provider should exist")
            .provider_operation(),
        NetavarkProviderOperation::Ready { .. }
    ));
    assert!(candidate.allocator().iter().all(|evidence| {
        evidence.observation().is_ok_and(|observation| {
            observation.state() == nimbus_network::NetworkAttachmentReservationState::Adopted
        })
    }));
}

#[test]
fn every_named_quarantine_reason_is_behaviorally_reachable() {
    let mut observed = BTreeMap::new();

    let (_fixture, mut no_desired) = ready_report("reason-no-desired", false);
    only_candidate(&mut no_desired).desired = None;
    observed.insert(
        "desired-missing",
        quarantine_reason(candidate_disposition(&no_desired)),
    );

    let (_fixture, mut no_provider) = ready_report("reason-no-provider", false);
    only_candidate(&mut no_provider).provider = None;
    observed.insert(
        "provider-missing",
        quarantine_reason(candidate_disposition(&no_provider)),
    );

    let (_fixture, terminal) = report_with_terminal_provider("reason-terminal");
    observed.insert(
        "provider-terminal",
        quarantine_reason(candidate_disposition(&terminal)),
    );

    let mismatched_provider = EvidenceFixture::new_with_selected_provider(
        "reason-backend-mismatch",
        AttachmentBackendKind::Container,
        SandboxAttachmentRegistrationKind::Krun,
        false,
    );
    let (_fixture, backend_mismatch) =
        report_with_ready_provider(mismatched_provider, NetworkResourcePhase::Ready);
    observed.insert(
        "provider-backend-mismatch",
        quarantine_reason(candidate_disposition(&backend_mismatch)),
    );

    let (_fixture, stale) = ready_report("reason-stale", true);
    observed.insert(
        "stale-generation",
        quarantine_reason(candidate_disposition(&stale)),
    );

    let reserved_desired = EvidenceFixture::new(
        "reason-desired-phase",
        AttachmentBackendKind::Container,
        false,
    );
    let (_fixture, desired_phase) =
        report_with_ready_provider(reserved_desired, NetworkResourcePhase::Reserved);
    observed.insert(
        "desired-phase",
        quarantine_reason(candidate_disposition(&desired_phase)),
    );

    let missing_handle = EvidenceFixture::new(
        "reason-missing-handle",
        AttachmentBackendKind::Container,
        false,
    );
    let (_fixture, missing_handle) =
        report_with_ready_provider_handle(missing_handle, NetworkResourcePhase::Ready, false);
    observed.insert(
        "desired-provider-handle-missing",
        quarantine_reason(candidate_disposition(&missing_handle)),
    );

    let (_fixture, substituted_handle) = report_with_substituted_provider_handle(
        "reason-substituted-handle",
        NetworkResourcePhase::Ready,
    );
    observed.insert(
        "desired-provider-handle-mismatch",
        quarantine_reason(candidate_disposition(&substituted_handle)),
    );

    let (_fixture, mut allocator_incomplete) = ready_report("reason-allocator-incomplete", false);
    only_candidate(&mut allocator_incomplete).allocator.pop();
    observed.insert(
        "allocator-incomplete",
        quarantine_reason(candidate_disposition(&allocator_incomplete)),
    );

    let (_fixture, mut no_hold) = ready_report("reason-no-hold", false);
    for allocator in &mut only_candidate(&mut no_hold).allocator {
        allocator.observation = Ok(NetworkAttachmentReservationObservation::absent());
    }
    observed.insert(
        "allocator-hold-missing",
        quarantine_reason(candidate_disposition(&no_hold)),
    );

    let (_fixture, mut unadopted) = ready_report("reason-unadopted", false);
    let association = only_candidate(&mut unadopted)
        .desired()
        .expect("desired should exist")
        .association()
        .clone();
    for allocator in &mut only_candidate(&mut unadopted).allocator {
        allocator.observation = Ok(NetworkAttachmentReservationObservation::bound_reserved(
            association.clone(),
        ));
    }
    observed.insert(
        "allocator-unadopted",
        quarantine_reason(candidate_disposition(&unadopted)),
    );

    let (_fixture, mut cleanup_pending) = ready_report("reason-cleanup-pending", false);
    let association = only_candidate(&mut cleanup_pending)
        .desired()
        .expect("desired should exist")
        .association()
        .clone();
    for allocator in &mut only_candidate(&mut cleanup_pending).allocator {
        allocator.observation = Ok(
            NetworkAttachmentReservationObservation::provider_cleanup_pending(association.clone()),
        );
    }
    observed.insert(
        "allocator-cleanup-pending",
        quarantine_reason(candidate_disposition(&cleanup_pending)),
    );

    let (_fixture, prepared) = report_with_prepared_provider("reason-provider-incomplete");
    observed.insert(
        "provider-effect-incomplete",
        quarantine_reason(candidate_disposition(&prepared)),
    );

    let (_fixture, mut artifact_incomplete) = ready_report("reason-artifact-incomplete", false);
    only_candidate(&mut artifact_incomplete)
        .artifacts
        .retain(|artifact| artifact.kind != OciArtifactKind::Status);
    observed.insert(
        "artifact-incomplete",
        quarantine_reason(candidate_disposition(&artifact_incomplete)),
    );

    let (_fixture, mut no_netns) = ready_report("reason-no-netns", false);
    only_candidate(&mut no_netns)
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.kind == OciArtifactKind::NetworkNamespace)
        .expect("netns should exist")
        .state = OciArtifactObservationState::Absent;
    observed.insert(
        "network-namespace-missing",
        quarantine_reason(candidate_disposition(&no_netns)),
    );

    let (_fixture, mut no_status) = ready_report("reason-no-status", false);
    only_candidate(&mut no_status)
        .artifacts
        .iter_mut()
        .find(|artifact| artifact.kind == OciArtifactKind::Status)
        .expect("status should exist")
        .state = OciArtifactObservationState::Absent;
    observed.insert(
        "provider-status-missing",
        quarantine_reason(candidate_disposition(&no_status)),
    );

    let (_fixture, mut unknown) = ready_report("reason-unknown", false);
    only_candidate(&mut unknown).allocator[0].observation = Err(OciEvidenceUnknown::domain(
        "inspect exact allocator reservation",
        "forced unknown",
    ));
    observed.insert(
        "unknown-inspection",
        quarantine_reason(candidate_disposition(&unknown)),
    );

    let realm_fixture =
        EvidenceFixture::new("reason-realm", AttachmentBackendKind::Container, false);
    let other_root = realm_fixture._temp_dir.path().join("foreign-realm");
    fs::create_dir_all(&other_root).expect("foreign realm should exist");
    let realm_report = collect_oci_orphan_evidence(
        &other_root,
        &realm_fixture.attachments,
        &realm_fixture.ipam,
        &realm_fixture.allocator,
    )
    .expect("foreign realm should remain evidence");
    let realm_classification = classify_oci_orphan_evidence(&realm_report);
    let [realm_classification] = realm_classification.unmatched_provider_classifications() else {
        panic!("one foreign provider realm should classify");
    };
    observed.insert(
        "provider-realm-mismatch",
        quarantine_reason(realm_classification.disposition()),
    );

    let (_fixture, mut unmatched_artifact) = ready_report("reason-unmatched-artifact", false);
    let artifact = only_candidate(&mut unmatched_artifact)
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == OciArtifactKind::Manifest)
        .expect("manifest should exist")
        .clone();
    unmatched_artifact.unmatched_artifacts.push(artifact);
    let unmatched_classification = classify_oci_orphan_evidence(&unmatched_artifact);
    let [unmatched_classification] = unmatched_classification.unmatched_artifact_classifications()
    else {
        panic!("one unmatched artifact should classify");
    };
    observed.insert(
        "unmatched-artifact",
        quarantine_reason(unmatched_classification.disposition()),
    );

    let expected = BTreeMap::from([
        (
            "desired-missing",
            OciOrphanQuarantineReason::DesiredAttachmentMissing,
        ),
        (
            "provider-missing",
            OciOrphanQuarantineReason::ProviderAttemptMissing,
        ),
        (
            "provider-terminal",
            OciOrphanQuarantineReason::ProviderAttemptTerminal,
        ),
        (
            "provider-backend-mismatch",
            OciOrphanQuarantineReason::ProviderBackendMismatch,
        ),
        (
            "stale-generation",
            OciOrphanQuarantineReason::StaleGenerationEvidence,
        ),
        (
            "desired-phase",
            OciOrphanQuarantineReason::DesiredPhaseNotAdoptable,
        ),
        (
            "desired-provider-handle-missing",
            OciOrphanQuarantineReason::DesiredProviderHandleMissing,
        ),
        (
            "desired-provider-handle-mismatch",
            OciOrphanQuarantineReason::DesiredProviderHandleMismatch,
        ),
        (
            "allocator-incomplete",
            OciOrphanQuarantineReason::AllocatorEvidenceIncomplete,
        ),
        (
            "allocator-hold-missing",
            OciOrphanQuarantineReason::AllocatorHoldMissing,
        ),
        (
            "allocator-unadopted",
            OciOrphanQuarantineReason::AllocatorReservationUnadopted,
        ),
        (
            "allocator-cleanup-pending",
            OciOrphanQuarantineReason::AllocatorCleanupPending,
        ),
        (
            "provider-effect-incomplete",
            OciOrphanQuarantineReason::ProviderEffectIncomplete,
        ),
        (
            "artifact-incomplete",
            OciOrphanQuarantineReason::ArtifactEvidenceIncomplete,
        ),
        (
            "network-namespace-missing",
            OciOrphanQuarantineReason::NetworkNamespaceMissing,
        ),
        (
            "provider-status-missing",
            OciOrphanQuarantineReason::ProviderStatusMissing,
        ),
        (
            "unknown-inspection",
            OciOrphanQuarantineReason::UnknownInspection,
        ),
        (
            "provider-realm-mismatch",
            OciOrphanQuarantineReason::ProviderRealmMismatch,
        ),
        (
            "unmatched-artifact",
            OciOrphanQuarantineReason::UnmatchedArtifact,
        ),
    ]);
    assert_eq!(observed, expected);
    assert_eq!(
        observed.values().copied().collect::<BTreeSet<_>>().len(),
        19,
        "every closed quarantine reason must have a distinct behavioral witness"
    );
}

#[test]
fn all_current_desired_phases_adopt_only_with_exact_ready_evidence() {
    for phase in [
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
    ] {
        let label = format!("current-phase-{phase:?}").to_ascii_lowercase();
        let fixture = EvidenceFixture::new(&label, AttachmentBackendKind::Krun, false);
        let (_fixture, report) = report_with_ready_provider(fixture, phase);
        assert_eq!(
            candidate_disposition(&report),
            OciOrphanDisposition::Adopt,
            "exact ready provider evidence should remain current in {phase:?}"
        );
    }
}

#[test]
fn substituted_provider_handle_cannot_adopt_current_generation() {
    for phase in [
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
    ] {
        let label = format!("substituted-provider-handle-{phase:?}").to_ascii_lowercase();
        let (_fixture, report) = report_with_substituted_provider_handle(&label, phase);
        assert_eq!(
            candidate_disposition(&report),
            quarantine(OciOrphanQuarantineReason::DesiredProviderHandleMismatch),
            "a same-provider handle from another realization must not authenticate in {phase:?}"
        );
    }
}

#[test]
fn missing_provider_handle_is_allowed_only_while_provisioning() {
    for phase in [
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
    ] {
        let label = format!("missing-provider-handle-{phase:?}").to_ascii_lowercase();
        let fixture = EvidenceFixture::new(&label, AttachmentBackendKind::Container, false);
        let (_fixture, report) = report_with_ready_provider_handle(fixture, phase, false);
        let expected = if phase == NetworkResourcePhase::Provisioning {
            OciOrphanDisposition::Adopt
        } else {
            quarantine(OciOrphanQuarantineReason::DesiredProviderHandleMissing)
        };
        assert_eq!(
            candidate_disposition(&report),
            expected,
            "handle presence rule must be exact for {phase:?}"
        );
    }
}

#[test]
fn desired_plan_id_generation_and_digest_must_match_the_live_plan_compiler() {
    let (fixture, report) = ready_report("desired-version-substitution", false);
    let expected = oci_attachment_plan(
        &fixture.tenant_id,
        &fixture.sandbox_id,
        AttachmentBackendKind::Container,
    );
    let requirements =
        host_managed_attachment_requirements(SandboxAttachmentRegistrationKind::Container);
    let substitutions = [
        (
            "plan-id",
            NetworkPlan::new(
                NetworkPlanId::for_tenant_workload_plan(
                    &fixture.tenant_id,
                    "foreign-workload-incarnation",
                ),
                expected.generation(),
                expected.content_digest(),
                requirements.clone(),
            ),
        ),
        (
            "generation",
            NetworkPlan::new(
                expected.plan_id().clone(),
                NetworkResourceGeneration::new(expected.generation().as_u64() + 1),
                expected.content_digest(),
                requirements.clone(),
            ),
        ),
        (
            "digest",
            NetworkPlan::new(
                expected.plan_id().clone(),
                expected.generation(),
                NetworkPlanContentDigest::sha256(b"foreign attachment content"),
                requirements,
            ),
        ),
    ];

    for (suffix, substituted_plan) in substitutions {
        let mut substituted = report.clone();
        only_candidate(&mut substituted).desired = Some(reserve_substituted_desired(
            &fixture,
            suffix,
            &substituted_plan,
        ));
        assert_eq!(
            candidate_disposition(&substituted),
            quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence),
            "{suffix} substitution must fail before adoption"
        );
    }
}

#[test]
fn every_report_collection_is_classified_in_order_and_unknowns_fail_closed() {
    let (fixture, mut report) = ready_report("report-collections", false);
    let (second_fixture, second_report) = ready_report("report-second-candidate", false);
    report.candidates.extend(second_report.candidates);

    let realm_fixture = EvidenceFixture::new(
        "report-foreign-provider",
        AttachmentBackendKind::Container,
        false,
    );
    let other_root = realm_fixture._temp_dir.path().join("other-realm");
    fs::create_dir_all(&other_root).expect("foreign realm should exist");
    let foreign = collect_oci_orphan_evidence(
        &other_root,
        &realm_fixture.attachments,
        &realm_fixture.ipam,
        &realm_fixture.allocator,
    )
    .expect("foreign provider should remain unmatched");
    report.unmatched_provider_evidence.push(
        foreign
            .unmatched_provider_evidence
            .into_iter()
            .next()
            .expect("one foreign provider should exist"),
    );

    let mut unmatched_artifact = report.candidates[0]
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == OciArtifactKind::Manifest)
        .expect("manifest should exist")
        .clone();
    unmatched_artifact.path = report.candidates[0].artifacts[0]
        .path
        .with_file_name("unmatched-manifest");
    report.unmatched_artifacts.push(unmatched_artifact);
    report
        .artifact_scan_unknowns
        .push(OciEvidenceUnknown::domain(
            "enumerate artifact root",
            "forced report-level scan failure",
        ));

    let before = report.clone();
    let authority_before = fixture.authority_bytes();
    let second_authority_before = second_fixture.authority_bytes();
    let first = classify_oci_orphan_evidence(&report);
    let second = classify_oci_orphan_evidence(&report);
    assert_eq!(first, second, "classification must be deterministic");
    assert_eq!(
        report, before,
        "classification must not mutate any evidence collection"
    );
    assert_eq!(
        fixture.authority_bytes(),
        authority_before,
        "classification must not mutate the first durable authority"
    );
    assert_eq!(
        second_fixture.authority_bytes(),
        second_authority_before,
        "classification must not mutate the second durable authority"
    );
    assert_eq!(
        first.candidate_classifications().len(),
        report.candidates().len()
    );
    assert_eq!(
        first.unmatched_provider_classifications().len(),
        report.unmatched_provider_evidence().len()
    );
    assert_eq!(
        first.unmatched_artifact_classifications().len(),
        report.unmatched_artifacts().len()
    );
    assert_eq!(
        first.artifact_scan_unknown_classifications().len(),
        report.artifact_scan_unknowns().len()
    );
    assert!(
        first
            .candidate_classifications()
            .iter()
            .zip(report.candidates())
            .all(|(classification, evidence)| std::ptr::eq(classification.evidence(), evidence))
    );
    assert!(std::ptr::eq(
        first.unmatched_provider_classifications()[0].evidence(),
        &report.unmatched_provider_evidence()[0]
    ));
    assert!(std::ptr::eq(
        first.unmatched_artifact_classifications()[0].evidence(),
        &report.unmatched_artifacts()[0]
    ));
    assert!(std::ptr::eq(
        first.artifact_scan_unknown_classifications()[0].evidence(),
        &report.artifact_scan_unknowns()[0]
    ));
    assert_eq!(
        dispositions(first.candidate_classifications()),
        [OciOrphanDisposition::Adopt, OciOrphanDisposition::Adopt]
    );
    assert_eq!(
        dispositions(first.unmatched_provider_classifications()),
        [quarantine(OciOrphanQuarantineReason::ProviderRealmMismatch)]
    );
    assert_eq!(
        dispositions(first.unmatched_artifact_classifications()),
        [quarantine(OciOrphanQuarantineReason::UnmatchedArtifact)]
    );
    assert_eq!(
        dispositions(first.artifact_scan_unknown_classifications()),
        [quarantine(OciOrphanQuarantineReason::UnknownInspection)]
    );

    report.unmatched_provider_evidence[0].realm =
        OciProviderRealmObservation::Unknown(OciEvidenceUnknown::domain(
            "authenticate artifact realm",
            "forced realm identity failure",
        ));
    report.unmatched_artifacts[0].state =
        OciArtifactObservationState::Unknown(OciEvidenceUnknown::domain(
            "inspect unmatched artifact",
            "forced unmatched artifact failure",
        ));
    let unknown = classify_oci_orphan_evidence(&report);
    assert_eq!(
        dispositions(unknown.unmatched_provider_classifications()),
        [quarantine(OciOrphanQuarantineReason::UnknownInspection)]
    );
    assert_eq!(
        dispositions(unknown.unmatched_artifact_classifications()),
        [quarantine(OciOrphanQuarantineReason::UnknownInspection)]
    );
}

#[test]
fn unsafe_reason_precedence_is_deterministic_and_path_never_becomes_identity() {
    let (_fixture, mut missing_desired) = ready_report("precedence-missing-desired", false);
    let candidate = only_candidate(&mut missing_desired);
    candidate.desired = None;
    candidate.artifacts[0].state = OciArtifactObservationState::Unknown(
        OciEvidenceUnknown::domain("inspect exact artifact", "lower-priority unknown"),
    );
    assert_eq!(
        candidate_disposition(&missing_desired),
        quarantine(OciOrphanQuarantineReason::DesiredAttachmentMissing)
    );

    let (_fixture, mut stale) = ready_report("precedence-stale", true);
    only_candidate(&mut stale).allocator[0].observation = Err(OciEvidenceUnknown::domain(
        "inspect exact allocator reservation",
        "lower-priority unknown",
    ));
    assert_eq!(
        candidate_disposition(&stale),
        quarantine(OciOrphanQuarantineReason::StaleGenerationEvidence)
    );

    let (_fixture, mut unknown) = ready_report("precedence-unknown", false);
    let candidate = only_candidate(&mut unknown);
    for allocator in &mut candidate.allocator {
        allocator.observation = Ok(NetworkAttachmentReservationObservation::absent());
    }
    candidate.artifacts[0].state =
        OciArtifactObservationState::Unknown(OciEvidenceUnknown::domain(
            "inspect exact artifact",
            "unknown outranks a known missing hold",
        ));
    assert_eq!(
        candidate_disposition(&unknown),
        quarantine(OciOrphanQuarantineReason::UnknownInspection)
    );

    let (_fixture, mut path_substitution) = ready_report("precedence-path", false);
    for (index, artifact) in only_candidate(&mut path_substitution)
        .artifacts
        .iter_mut()
        .enumerate()
    {
        artifact.path = artifact
            .path
            .with_file_name(format!("untrusted-path-{index}"));
    }
    assert_eq!(
        candidate_disposition(&path_substitution),
        OciOrphanDisposition::Adopt,
        "NNC5.2b authenticates candidate association before classification; paths never select identity"
    );
}

#[test]
fn classifier_source_has_no_io_effect_or_mutation_capability() {
    let classifier_source = include_str!("../classifier.rs");
    let plan_compiler_source = include_str!("../../attachment_lifecycle/plan.rs");
    for forbidden in [
        "std::fs",
        "cap_std",
        "std::net",
        "TcpListener",
        "UdpSocket",
        "Command::",
        "OciIpamAuthority",
        "LocalNetworkAttachmentAuthority",
        "NetworkSegmentAllocator",
        "begin_netavark_",
        "complete_netavark_",
        ".quarantine(",
        ".release(",
        ".finalize_",
        "reconcile_",
        "remove_file",
        "remove_dir",
        "unsafe {",
        ".expect(",
        ".unwrap(",
        "#[allow(",
    ] {
        assert!(
            !classifier_source.contains(forbidden),
            "pure classifier must not contain forbidden capability token {forbidden:?}",
        );
        assert!(
            !plan_compiler_source.contains(forbidden),
            "pure desired-plan compiler must not contain forbidden capability token {forbidden:?}",
        );
    }
    assert!(
        !classifier_source.contains("Remove,"),
        "the closed disposition vocabulary must not contain a remove result"
    );
}
