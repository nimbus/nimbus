use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkAttachmentSegmentAssociation, NetworkCapabilitySourceDigest,
    NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest, NetworkPlanId, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkSegmentId,
};

use super::*;
use crate::backends::{
    CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY, sandbox_network_plan_requirements,
};
use crate::{
    ProviderCommandAttemptJournal, ProviderCommandClaimDecision, ProviderCommandClaimInput,
    SandboxBackendKind, SandboxNetworkTeardownCommandInput, SandboxNetworkTeardownIdentity,
    SandboxNetworkTeardownIdentityInput,
};

const BASE_EPOCH: u64 = 7;

fn plan() -> NetworkPlan {
    let tenant_id = TenantId::new("tenant-a").expect("tenant fixture should validate");
    NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a"),
        NetworkResourceGeneration::new(3),
        NetworkPlanContentDigest::sha256(b"host teardown state plan"),
        sandbox_network_plan_requirements(SandboxBackendKind::Container)
            .capability_requirements()
            .clone(),
    )
}

fn identity() -> SandboxNetworkTeardownIdentity {
    SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: TenantId::new("tenant-a").expect("tenant fixture should validate"),
        sandbox_id: SandboxId::new("sandbox-a"),
        execution_attempt_id: SandboxExecutionAttemptId::new("execution-attempt-a")
            .expect("execution attempt fixture should validate"),
        attachment_id: NetworkAttachmentId::for_workload_attachment("workload-a", "default"),
        network_plan: plan(),
        provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([3; 32]),
    })
    .expect("teardown identity fixture should validate")
}

fn command_with_attempt(
    operation: SandboxNetworkTeardownOperation,
    dispatch_epoch: u64,
    attempt_id: &str,
) -> SandboxNetworkTeardownCommand {
    let identity = identity();
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: "network-authority-a".to_owned(),
        effect_subject: identity.provider_effect_subject(),
        source_attempt_id: None,
        attempt_id: attempt_id.to_owned(),
        dispatch_epoch,
        workload_generation: identity.network_plan().generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: "1".repeat(64),
        source_digest: "2".repeat(64),
        network_plan_digest: identity.network_plan().digest().to_string(),
        provider_target_digest: identity.provider_target_digest(),
        operation: operation.provider_operation(),
    })
    .expect("provider claim fixture should validate");
    SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim: claim,
    })
    .expect("teardown command fixture should validate")
}

fn command(
    operation: SandboxNetworkTeardownOperation,
    dispatch_epoch: u64,
) -> SandboxNetworkTeardownCommand {
    command_with_attempt(operation, dispatch_epoch, "network-attempt-a")
}

fn association() -> NetworkAttachmentSegmentAssociation {
    NetworkAttachmentSegmentAssociation::new(
        NetworkReservationClaim::new(
            NetworkProviderHandle::new(
                NetworkProviderId::for_registration_key("nimbus.test.attachment-coordinator"),
                "reservation-attempt-a",
            )
            .expect("provider handle fixture should validate"),
        ),
        "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAV"
            .parse::<NetworkSegmentId>()
            .expect("segment fixture should parse"),
        NetworkLeaseEpoch::new(11),
    )
}

fn detached_proof(
    detach_command: SandboxNetworkTeardownCommand,
) -> HostManagedAttachmentDetachedProof {
    let selected_provider_id = detach_command.provider_id();
    HostManagedAttachmentDetachedProof::new(HostManagedAttachmentDetachedProofInput {
        command: detach_command,
        association: association(),
        selected_provider_id,
        stable_handle_sha256: "a".repeat(64),
        provider_delete_evidence_sha256: "b".repeat(64),
        namespace_absence_evidence_sha256: "c".repeat(64),
        pep_retained_evidence_sha256: "d".repeat(64),
        listener_retained_evidence_sha256: "e".repeat(64),
        ipam_retained_evidence_sha256: "f".repeat(64),
        segment_quarantine_evidence_sha256: "1".repeat(64),
        attachment_retained_evidence_sha256: "2".repeat(64),
    })
    .expect("detached proof fixture should validate")
}

fn partial_detach_state(
    command: &SandboxNetworkTeardownCommand,
) -> HostManagedAttachmentTeardownState {
    HostManagedAttachmentTeardownState {
        detach_claim: Some(command.provider_claim().clone()),
        detach_phase: HostManagedAttachmentDetachPhase::ProviderDeleteMayExist,
        detached_proof: None,
        release_claim: None,
        release_phase: HostManagedAttachmentReleasePhase::NotStarted,
    }
}

fn detached_state(
    detach_command: SandboxNetworkTeardownCommand,
) -> HostManagedAttachmentTeardownState {
    let detach_claim = detach_command.provider_claim().clone();
    HostManagedAttachmentTeardownState {
        detach_claim: Some(detach_claim),
        detach_phase: HostManagedAttachmentDetachPhase::Detached,
        detached_proof: Some(detached_proof(detach_command)),
        release_claim: None,
        release_phase: HostManagedAttachmentReleasePhase::NotStarted,
    }
}

fn partial_release_state(
    detach_command: SandboxNetworkTeardownCommand,
    release_command: &SandboxNetworkTeardownCommand,
) -> HostManagedAttachmentTeardownState {
    let mut state = detached_state(detach_command);
    state.release_claim = Some(release_command.provider_claim().clone());
    state.release_phase = HostManagedAttachmentReleasePhase::IpamReleaseMayExist;
    state
}

fn claimed_observation(claim: &ProviderCommandClaim) -> ProviderCommandObservation {
    let root = tempfile::tempdir().expect("temporary journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "state-inspection-test")
        .expect("provider journal fixture should open");
    match journal
        .claim_dispatch_epoch(claim)
        .expect("current claim fixture should persist")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution.observation().clone(),
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("a fresh journal must grant execute authority")
        }
    }
}

fn nonterminal_observation(
    claim: &ProviderCommandClaim,
    kind: ProviderCommandObservationKind,
) -> ProviderCommandObservation {
    let root = tempfile::tempdir().expect("temporary journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "state-nonterminal-test")
        .expect("provider journal fixture should open");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(claim)
            .expect("current claim fixture should persist"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    journal
        .record_observation(claim, kind, b"provider effect remains nonterminal")
        .expect("nonterminal observation fixture should persist")
}

fn adjacent_retry_observation(
    stored: &ProviderCommandClaim,
    current: &ProviderCommandClaim,
) -> ProviderCommandObservation {
    let root = tempfile::tempdir().expect("temporary journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "state-rebase-test")
        .expect("provider journal fixture should open");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(stored)
            .expect("stored claim fixture should persist"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    journal
        .record_observation(
            stored,
            ProviderCommandObservationKind::Absent,
            b"provider proved the stored teardown epoch absent",
        )
        .expect("retry-authorizing absence should persist");
    match journal
        .claim_dispatch_epoch(current)
        .expect("adjacent retry fixture should persist")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution.observation().clone(),
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("an adjacent retry must grant execute authority")
        }
    }
}

#[test]
fn detach_rebase_changes_only_the_selected_claim() {
    let stored = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let current = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH + 1);
    let observation = adjacent_retry_observation(stored.provider_claim(), current.provider_claim());
    let mut state = partial_detach_state(&stored);
    let before = state.clone();

    assert_eq!(
        state.inspect_and_rebase_command(&current, &observation),
        Ok(HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor)
    );
    assert_eq!(state.detach_claim.as_ref(), Some(current.provider_claim()));
    assert_eq!(state.detach_phase, before.detach_phase);
    assert_eq!(state.detached_proof, before.detached_proof);
    assert_eq!(state.release_claim, before.release_claim);
    assert_eq!(state.release_phase, before.release_phase);
}

#[test]
fn release_rebase_changes_only_the_selected_claim() {
    let detach = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let stored = command(SandboxNetworkTeardownOperation::Release, BASE_EPOCH);
    let current = command(SandboxNetworkTeardownOperation::Release, BASE_EPOCH + 1);
    let observation = adjacent_retry_observation(stored.provider_claim(), current.provider_claim());
    let mut state = partial_release_state(detach, &stored);
    let before = state.clone();

    assert_eq!(
        state.inspect_and_rebase_command(&current, &observation),
        Ok(HostManagedAttachmentCommandInspection::AuthorizedImmediatePredecessor)
    );
    assert_eq!(state.release_claim.as_ref(), Some(current.provider_claim()));
    assert_eq!(state.release_phase, before.release_phase);
    assert_eq!(state.detach_claim, before.detach_claim);
    assert_eq!(state.detach_phase, before.detach_phase);
    assert_eq!(state.detached_proof, before.detached_proof);
}

#[test]
fn exact_partial_replay_is_byte_stable() {
    let current = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let observation = claimed_observation(current.provider_claim());
    let mut state = partial_detach_state(&current);
    let before = serde_json::to_vec(&state).expect("state fixture should serialize");

    assert_eq!(
        state.inspect_and_rebase_command(&current, &observation),
        Ok(HostManagedAttachmentCommandInspection::ExactCurrentPartial)
    );
    assert_eq!(
        serde_json::to_vec(&state).expect("replayed state should serialize"),
        before
    );
}

#[test]
fn exact_nonterminal_inspection_is_read_only_with_or_without_progress() {
    let current = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);

    for kind in [
        ProviderCommandObservationKind::InProgress,
        ProviderCommandObservationKind::Ambiguous,
    ] {
        let observation = nonterminal_observation(current.provider_claim(), kind);
        let mut partial = partial_detach_state(&current);
        let partial_before = serde_json::to_vec(&partial).expect("partial state should serialize");
        assert_eq!(
            partial.inspect_and_rebase_command(&current, &observation),
            Ok(HostManagedAttachmentCommandInspection::ExactCurrentPartial)
        );
        assert_eq!(
            serde_json::to_vec(&partial).expect("inspected partial state should serialize"),
            partial_before
        );

        let mut initial = HostManagedAttachmentTeardownState::initial();
        let initial_before = serde_json::to_vec(&initial).expect("initial state should serialize");
        assert_eq!(
            initial.inspect_and_rebase_command(&current, &observation),
            Ok(HostManagedAttachmentCommandInspection::ExactCurrentPartial)
        );
        assert_eq!(
            serde_json::to_vec(&initial).expect("inspected initial state should serialize"),
            initial_before
        );
    }
}

#[test]
fn nonadjacent_and_crossed_replays_are_rejected_without_writes() {
    let stored = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let mut state = partial_detach_state(&stored);
    let before = state.clone();

    let skipped = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH + 2);
    assert_eq!(
        state.inspect_and_rebase_command(&skipped, &claimed_observation(skipped.provider_claim())),
        Err(HostManagedAttachmentCommandInspectionError::EpochInvalid)
    );
    assert_eq!(state, before);

    let crossed = command_with_attempt(
        SandboxNetworkTeardownOperation::Detach,
        BASE_EPOCH + 1,
        "crossed-network-attempt",
    );
    assert_eq!(
        state.inspect_and_rebase_command(&crossed, &claimed_observation(crossed.provider_claim())),
        Err(HostManagedAttachmentCommandInspectionError::Crossed)
    );
    assert_eq!(state, before);
}

#[test]
fn detached_and_released_state_reject_adjacent_rebase() {
    let detach_stored = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let detach_current = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH + 1);
    let mut detach_state = detached_state(detach_stored.clone());
    let detach_before = serde_json::to_vec(&detach_state).expect("detached state should serialize");
    assert_eq!(
        detach_state.inspect_and_rebase_command(
            &detach_stored,
            &claimed_observation(detach_stored.provider_claim())
        ),
        Ok(HostManagedAttachmentCommandInspection::ExactTerminalSuccess)
    );
    assert_eq!(
        detach_state.inspect_and_rebase_command(
            &detach_current,
            &adjacent_retry_observation(
                detach_stored.provider_claim(),
                detach_current.provider_claim()
            )
        ),
        Err(HostManagedAttachmentCommandInspectionError::EpochInvalid)
    );
    assert_eq!(
        serde_json::to_vec(&detach_state).expect("rejected detach should serialize"),
        detach_before
    );

    let release_stored = command(SandboxNetworkTeardownOperation::Release, BASE_EPOCH);
    let release_current = command(SandboxNetworkTeardownOperation::Release, BASE_EPOCH + 1);
    let mut release_state = partial_release_state(detach_stored, &release_stored);
    release_state.release_phase = HostManagedAttachmentReleasePhase::Released;
    let release_before = release_state.clone();
    assert_eq!(
        release_state.inspect_and_rebase_command(
            &release_current,
            &adjacent_retry_observation(
                release_stored.provider_claim(),
                release_current.provider_claim()
            )
        ),
        Err(HostManagedAttachmentCommandInspectionError::EpochInvalid)
    );
    assert_eq!(release_state, release_before);
}

#[test]
fn adjacent_claim_without_authenticated_lineage_is_corrupt() {
    let stored = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH);
    let current = command(SandboxNetworkTeardownOperation::Detach, BASE_EPOCH + 1);
    let mut state = partial_detach_state(&stored);
    let before = state.clone();

    assert_eq!(
        state.inspect_and_rebase_command(&current, &claimed_observation(current.provider_claim())),
        Err(HostManagedAttachmentCommandInspectionError::Corrupt)
    );
    assert_eq!(state, before);
}
