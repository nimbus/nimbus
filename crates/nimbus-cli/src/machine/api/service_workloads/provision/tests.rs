use nimbus_core::{TenantId, WorkloadId};
use nimbus_machine::api::{
    MachineApiWorkloadProvisionCommandEnvelope, MachineApiWorkloadProvisionObservation,
    MachineApiWorkloadProvisionPhaseRequest,
};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkCapabilitySelectionEvidence, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, NodeIdentity, TenantWorkloadUid, WorkloadActivationIntent,
    WorkloadDesiredDigest, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadExecutionAttemptId, WorkloadExecutionId, WorkloadExecutionReference,
    WorkloadGeneration, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadNetworkReference, WorkloadOwnerEvidenceDigest, WorkloadProvisionAttempt,
    WorkloadProvisionAttemptInput, WorkloadProvisionCommandId, WorkloadProvisionCommandMode,
    WorkloadProvisionDispatchClaim, WorkloadProvisionPrerequisiteEvidence,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadPublicationReference,
    WorkloadRestartEpoch, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRevision,
    WorkloadSagaTransitionId,
};

use crate::machine::api::tests::test_forwarder_authority;

#[test]
fn every_provision_step_maps_to_one_exact_provider_journal_operation() {
    use nimbus_sandbox::ProviderCommandOperation;

    let cases = [
        (
            WorkloadProvisionStep::ReserveNetwork,
            ProviderCommandOperation::ReserveNetwork,
        ),
        (
            WorkloadProvisionStep::PrepareWorkload,
            ProviderCommandOperation::PrepareWorkload,
        ),
        (
            WorkloadProvisionStep::AttachNetwork,
            ProviderCommandOperation::AttachNetwork,
        ),
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            ProviderCommandOperation::InspectActivationPrerequisites,
        ),
        (
            WorkloadProvisionStep::ActivateWorkload,
            ProviderCommandOperation::ActivateWorkload,
        ),
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            ProviderCommandOperation::InspectWorkloadReadiness,
        ),
        (
            WorkloadProvisionStep::Publish,
            ProviderCommandOperation::PublishIngress,
        ),
        (
            WorkloadProvisionStep::ObservePublication,
            ProviderCommandOperation::ObserveIngress,
        ),
    ];

    for (step, expected) in cases {
        assert_eq!(super::operation(step), expected, "{step:?}");
    }
}

pub(crate) fn request_fixture(
    suffix: char,
    step: WorkloadProvisionStep,
    mode: WorkloadProvisionCommandMode,
) -> MachineApiWorkloadProvisionPhaseRequest {
    request_fixture_with_attachment(suffix, step, mode, None)
}

fn request_fixture_with_attachment(
    suffix: char,
    step: WorkloadProvisionStep,
    mode: WorkloadProvisionCommandMode,
    attachment_provider: Option<NetworkProviderId>,
) -> MachineApiWorkloadProvisionPhaseRequest {
    let tenant_id =
        TenantId::new(format!("tenant-phase-{suffix}")).expect("fixture tenant should validate");
    let generation = WorkloadGeneration::new(1);
    let desired_digest = WorkloadDesiredDigest::sha256(format!("desired-{suffix}"));
    let node =
        NodeIdentity::new(format!("node-phase-{suffix}")).expect("fixture node should validate");
    let workload_uid: TenantWorkloadUid = format!("twu_{}", suffix.to_string().repeat(64))
        .try_into()
        .expect("fixture workload UID should validate");
    let execution_id = WorkloadExecutionId::for_execution(&workload_uid, &node, generation);
    let restart_epoch = WorkloadRestartEpoch::new(0);
    let attempt_id = WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch);
    let execution: WorkloadExecutionReference = serde_json::from_value(serde_json::json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node,
        "executionId": execution_id,
        "restartEpoch": restart_epoch,
        "attemptId": attempt_id,
        "generation": generation,
        "desiredDigest": desired_digest,
    }))
    .expect("fixture execution should validate");
    let compiled = compiled_network_plan_fixture(&tenant_id, suffix, generation);
    let network: WorkloadNetworkReference = serde_json::from_value(serde_json::json!({
        "planId": compiled.plan().plan_id(),
        "generation": generation.to_string(),
        "digest": compiled.plan().digest(),
    }))
    .expect("fixture network reference should validate");
    let publication: WorkloadPublicationReference = serde_json::from_value(serde_json::json!({
        "endpoints": [PublishedEndpointId::for_workload_endpoint(
            &format!("phase-{suffix}"),
            "http",
        )],
        "network": network,
        "execution": execution,
    }))
    .expect("fixture publication should validate");
    let execution_provider =
        crate::machine::backend::provision::forwarded_machine_execution_provider_id();
    let attachment_provider = attachment_provider.unwrap_or_else(|| {
        crate::machine::backend::provision::forwarded_machine_attachment_provider_id()
    });
    let authority = test_forwarder_authority(&format!("phase-forwarder-{suffix}"));
    let selection = selection_evidence(
        attachment_provider,
        authority.provider_instance().provider_id().clone(),
        suffix,
    );
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"phase-{suffix}"}}"#),
    )
    .expect("fixture executable carrier should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            format!("workload-phase-{suffix}"),
            format!("profile-phase-{suffix}"),
        )
        .expect("fixture source identity should validate"),
        WorkloadProvisionSourceGeneration::new(1),
        WorkloadProvisionSourceResourceVersion::new(format!("source-version-{suffix}"))
            .expect("fixture source version should validate"),
        executable.content_digest(),
        selection.selection().attachment_provider_id().clone(),
        execution_provider.clone(),
    )
    .expect("fixture source evidence should validate");
    let source_digest = source.source_digest();
    let (source_phase, target_phase) = phases(step);
    let subjects = subjects(step, &network, &execution, &publication);
    let prerequisite = (step == WorkloadProvisionStep::ActivateWorkload).then(|| {
        WorkloadProvisionPrerequisiteEvidence::new(
            format!("wpa_{}", "ef".repeat(32))
                .parse()
                .expect("fixture prerequisite attempt should validate"),
            WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                network: network.clone(),
                execution: execution.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("prerequisite-ready"),
            },
        )
        .expect("fixture prerequisite should validate")
    });
    let key = WorkloadSagaKey::new(
        tenant_id,
        WorkloadId::new(format!("workload-phase-{suffix}"))
            .expect("fixture workload ID should validate"),
    );
    let attempt = WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        saga_id: key.saga_id(),
        key,
        issuing_revision: WorkloadSagaRevision::new(0),
        generation,
        desired_digest,
        required_node: node,
        source_digest,
        execution_provider_id: execution_provider.clone(),
        network_plan_digest: compiled.plan().digest(),
        selection_evidence: Some(selection.clone()),
        source_phase,
        target_phase,
        step,
        subjects,
        prerequisite,
    })
    .expect("fixture attempt should validate");
    let provider_target = match step {
        WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork => {
            WorkloadProvisionProviderTarget::Network {
                role: nimbus_network::NetworkCapabilityRole::Attachment,
                provider_id: selection.selection().attachment_provider_id().clone(),
                provider_source_digest: selection.source_digest(),
            }
        }
        WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication => {
            WorkloadProvisionProviderTarget::Network {
                role: nimbus_network::NetworkCapabilityRole::Ingress,
                provider_id: selection.selection().ingress_provider_id().clone(),
                provider_source_digest: selection.source_digest(),
            }
        }
        _ => WorkloadProvisionProviderTarget::Execution {
            provider_id: execution_provider,
            provider_source_digest: source_digest,
        },
    };
    let claim: WorkloadProvisionDispatchClaim = serde_json::from_value(serde_json::json!({
        "attempt": attempt,
        "claimedRevision": "1",
        "dispatchEpoch": "0",
        "providerTarget": provider_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("fixture claim should validate");
    let transition_id: WorkloadSagaTransitionId = format!("wst_{}", suffix.to_string().repeat(64))
        .try_into()
        .expect("fixture transition should validate");
    let confirmed_revision = WorkloadSagaRevision::new(1);
    let command_id = WorkloadProvisionCommandId::for_confirmed_dispatch(
        &claim,
        confirmed_revision,
        &transition_id,
        &execution,
        mode,
    )
    .expect("fixture command ID should validate");
    let command = MachineApiWorkloadProvisionCommandEnvelope::new(
        command_id,
        claim.attempt().attempt_id().clone(),
        claim.dispatch_epoch(),
        claim.provider_target().clone(),
        claim,
        confirmed_revision,
        transition_id,
        generation,
        desired_digest,
        source,
        compiled.plan().digest(),
        execution,
        executable,
        compiled,
        authority.generation(),
        mode,
    )
    .expect("fixture command should validate");
    MachineApiWorkloadProvisionPhaseRequest::new(authority, command)
        .expect("fixture request should validate")
}

#[test]
fn guest_validation_rejects_wrong_node_and_provider_before_effect_inputs() {
    let wrong_node_request = request_fixture(
        'c',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let other_node = NodeIdentity::new("other-node").expect("other node should validate");
    assert!(matches!(
        super::validate_command(
            &other_node,
            wrong_node_request.command(),
            wrong_node_request.forwarder_authority(),
        ),
        Err(MachineApiWorkloadProvisionObservation::DefiniteFailure { .. })
    ));

    let wrong_provider_request = request_fixture_with_attachment(
        'd',
        WorkloadProvisionStep::ReserveNetwork,
        WorkloadProvisionCommandMode::Execute,
        Some(NetworkProviderId::for_registration_key(
            "foreign-attachment-provider",
        )),
    );
    let current_node = wrong_provider_request
        .command()
        .claim()
        .attempt()
        .required_node();
    assert!(matches!(
        super::validate_command(
            current_node,
            wrong_provider_request.command(),
            wrong_provider_request.forwarder_authority(),
        ),
        Err(MachineApiWorkloadProvisionObservation::DefiniteFailure { .. })
    ));
}

#[test]
fn readiness_running_is_in_progress_until_ready() {
    let running = super::host_phase_result(
        nimbus_node::TenantWorkloadPhase::Running,
        super::HostStatusSuccess::Ready,
        b"running".to_vec(),
    );
    assert!(matches!(
        running,
        MachineApiWorkloadProvisionObservation::InProgress { .. }
    ));

    let activated = super::host_phase_result(
        nimbus_node::TenantWorkloadPhase::Running,
        super::HostStatusSuccess::Activated,
        b"running".to_vec(),
    );
    assert!(matches!(
        activated,
        MachineApiWorkloadProvisionObservation::Succeeded { .. }
    ));

    let ready = super::host_phase_result(
        nimbus_node::TenantWorkloadPhase::Ready,
        super::HostStatusSuccess::Ready,
        b"ready".to_vec(),
    );
    assert!(matches!(
        ready,
        MachineApiWorkloadProvisionObservation::Succeeded { .. }
    ));
}

#[test]
fn only_process_bound_provision_effects_reconcile_live_absence() {
    assert!(super::live_reconciliation(WorkloadProvisionStep::Publish));
    assert!(super::live_reconciliation(
        WorkloadProvisionStep::ObservePublication
    ));
    for step in [
        WorkloadProvisionStep::ReserveNetwork,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionStep::AttachNetwork,
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadProvisionStep::ActivateWorkload,
        WorkloadProvisionStep::InspectWorkloadReadiness,
    ] {
        assert!(!super::live_reconciliation(step), "{step:?}");
    }
}

fn phases(step: WorkloadProvisionStep) -> (WorkloadSagaPhase, WorkloadSagaPhase) {
    match step {
        WorkloadProvisionStep::ReserveNetwork => (
            WorkloadSagaPhase::IntentCommitted,
            WorkloadSagaPhase::NetworkReserved,
        ),
        WorkloadProvisionStep::PrepareWorkload => (
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared,
        ),
        WorkloadProvisionStep::AttachNetwork => (
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached,
        ),
        WorkloadProvisionStep::InspectActivationPrerequisites => (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::NetworkAttached,
        ),
        WorkloadProvisionStep::ActivateWorkload => (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated,
        ),
        WorkloadProvisionStep::InspectWorkloadReadiness => (
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadSagaPhase::Ready,
        ),
        WorkloadProvisionStep::Publish => (WorkloadSagaPhase::Ready, WorkloadSagaPhase::Published),
        WorkloadProvisionStep::ObservePublication => {
            (WorkloadSagaPhase::Published, WorkloadSagaPhase::Observed)
        }
    }
}

fn subjects(
    step: WorkloadProvisionStep,
    network: &WorkloadNetworkReference,
    execution: &WorkloadExecutionReference,
    publication: &WorkloadPublicationReference,
) -> WorkloadProvisionSubjects {
    match step {
        WorkloadProvisionStep::ReserveNetwork | WorkloadProvisionStep::AttachNetwork => {
            WorkloadProvisionSubjects::Network(network.clone())
        }
        WorkloadProvisionStep::PrepareWorkload | WorkloadProvisionStep::ActivateWorkload => {
            WorkloadProvisionSubjects::Execution(execution.clone())
        }
        WorkloadProvisionStep::InspectActivationPrerequisites
        | WorkloadProvisionStep::InspectWorkloadReadiness => WorkloadProvisionSubjects::Readiness {
            network: network.clone(),
            execution: execution.clone(),
        },
        WorkloadProvisionStep::Publish | WorkloadProvisionStep::ObservePublication => {
            WorkloadProvisionSubjects::Publication(publication.clone())
        }
    }
}

fn selection_evidence(
    attachment: NetworkProviderId,
    ingress: NetworkProviderId,
    suffix: char,
) -> NetworkCapabilitySelectionEvidence {
    serde_json::from_value(serde_json::json!({
        "selection": NetworkCapabilitySelection::new(attachment, ingress),
        "source_digest": format!("{:02x}", suffix as u8).repeat(32),
    }))
    .expect("fixture selection should validate")
}

fn compiled_network_plan_fixture(
    tenant_id: &TenantId,
    suffix: char,
    generation: WorkloadGeneration,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("phase-incarnation-{suffix}"),
        NetworkResourceGeneration::new(generation.as_u64()),
    )
    .expect("fixture network identity should validate");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [],
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("fixture network content should validate");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixture compiled plan should validate")
}
