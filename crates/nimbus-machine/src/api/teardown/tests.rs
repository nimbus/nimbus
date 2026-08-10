use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration, NetworkCapabilityBundle,
    NetworkCapabilityRequirements, NetworkCapabilitySelection, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkProviderHandle, NetworkProviderId, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadDesiredDigest, WorkloadExecutableEncoding,
    WorkloadExecutableIntent, WorkloadExecutionProviderId, WorkloadGeneration,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkIntent, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadNetworkReference, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadPublicationReference, WorkloadRestartEpoch,
    WorkloadSagaIntent, WorkloadSagaKey, WorkloadTeardownAttempt, WorkloadTeardownAttemptInput,
    WorkloadTeardownCause, WorkloadTeardownReceipt, WorkloadTeardownResultConfirmation,
};
use serde_json::{Value, json};

use super::*;

#[test]
fn teardown_wire_constants_are_private_protocol_vocabulary() {
    assert_eq!(
        super::super::MACHINE_API_WORKLOAD_TEARDOWN_PHASE_PATH,
        "/v1/machine-api/workload-teardown/phase"
    );
    assert_eq!(
        super::super::MACHINE_API_WORKLOAD_TEARDOWN_PHASE_OPERATION,
        "workload-teardown.phase"
    );
}

#[test]
fn teardown_requests_round_trip_all_guest_steps_and_modes() {
    let mut digests = Vec::new();
    for step in guest_steps() {
        for mode in [
            WorkloadTeardownCommandMode::Execute,
            WorkloadTeardownCommandMode::Inspect,
        ] {
            let request = request_fixture('a', step, mode);
            let encoded = serde_json::to_value(&request).unwrap();
            let decoded: MachineApiWorkloadTeardownPhaseRequest =
                serde_json::from_value(encoded).unwrap();
            assert_eq!(decoded, request);
            assert_eq!(decoded.command().step(), step);
            assert_eq!(decoded.command().mode(), mode);
            assert_eq!(
                decoded.command().prior_receipt_prefix().receipts().len(),
                step_index(step)
            );
            digests.push(decoded.request_digest());
        }
    }
    digests.sort_by_key(ToString::to_string);
    digests.dedup();
    assert_eq!(digests.len(), 8, "every exact request has a unique digest");
}

#[test]
fn teardown_envelope_rejects_parent_local_withdrawal_and_incomplete_history() {
    let withdrawal = command_input_fixture(
        'a',
        WorkloadTeardownStep::WithdrawPublication,
        WorkloadTeardownCommandMode::Execute,
    );
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(withdrawal),
        Err(MachineApiWorkloadTeardownWireError::UnsupportedStep)
    );

    let mut stop = command_input_fixture(
        'a',
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let mut prefix = serde_json::to_value(&stop.prior_receipt_prefix).unwrap();
    prefix["receipts"].as_array_mut().unwrap().remove(0);
    stop.prior_receipt_prefix = serde_json::from_value(prefix).unwrap();
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(stop),
        Err(MachineApiWorkloadTeardownWireError::PriorReceiptChainIncomplete)
    );

    let fixture = TeardownFixture::new('a');
    let crossed_execution = WorkloadExecutionReference::for_restart_epoch(
        &fixture.intent,
        WorkloadRestartEpoch::new(1),
    );
    let receipts = vec![
        fixture.receipt(WorkloadTeardownStep::WithdrawPublication),
        fixture.receipt_with_subjects(
            WorkloadTeardownStep::DrainExecution,
            WorkloadTeardownSubjects::Execution(crossed_execution),
        ),
    ];
    let mut crossed_history = command_input_fixture(
        'a',
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    crossed_history.prior_receipt_prefix =
        serde_json::from_value(json!({ "receipts": receipts })).unwrap();
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(crossed_history),
        Err(MachineApiWorkloadTeardownWireError::PriorReceiptChainIncomplete),
        "a valid portable prefix with a crossed execution attempt must fail at the forwarded seam"
    );
}

#[test]
fn teardown_envelope_rejects_crossed_source_plan_translation_and_authority() {
    let mut source = command_input_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    source.source = command_input_fixture(
        'b',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    )
    .source;
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(source),
        Err(MachineApiWorkloadTeardownWireError::SourceDigestMismatch)
    );

    let mut plan = command_input_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    plan.compiled_network_plan = command_input_fixture(
        'b',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    )
    .compiled_network_plan;
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(plan),
        Err(MachineApiWorkloadTeardownWireError::TenantMismatch)
    );

    let mut translation = command_input_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    translation.provider_translation =
        MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment;
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(translation),
        Err(MachineApiWorkloadTeardownWireError::ProviderTranslationMismatch)
    );

    let mut authority = command_input_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    authority.machine_provider_generation = NetworkResourceGeneration::new(8);
    assert_eq!(
        MachineApiWorkloadTeardownCommandEnvelope::new(authority),
        Err(MachineApiWorkloadTeardownWireError::MachineProviderGenerationMismatch)
    );
}

#[test]
fn teardown_request_digest_and_strict_wire_reject_mutation() {
    let request = request_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let value = serde_json::to_value(&request).unwrap();

    for (pointer, replacement) in [
        (
            "/command/providerTranslation",
            json!("guest_container_attachment"),
        ),
        ("/command/mode", json!("inspect")),
        ("/command/confirmedRevision", json!("999")),
        ("/command/machineProviderGeneration", json!("8")),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            serde_json::from_value::<MachineApiWorkloadTeardownPhaseRequest>(changed).is_err(),
            "mutated request field {pointer} must fail closed"
        );
    }

    for pointer in ["", "/command", "/command/claim"] {
        let mut unknown = value.clone();
        unknown
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), json!(true));
        assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseRequest>(unknown).is_err());
    }

    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("requestDigest");
    assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseRequest>(missing).is_err());
    let mut null = value;
    null["command"]["providerTranslation"] = Value::Null;
    assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseRequest>(null).is_err());
}

#[test]
fn teardown_responses_round_trip_closed_mode_specific_outcomes() {
    let execute = request_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let inspect = request_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Inspect,
    );
    let success = success_evidence(
        WorkloadTeardownStep::DrainExecution,
        execute.command().subjects(),
        "execute-success",
    );
    let failure = WorkloadFailureEvidence::new(
        "machine_teardown_test_failure",
        WorkloadOwnerEvidenceDigest::sha256("failure"),
    )
    .unwrap();
    let execute_outcomes = [
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Succeeded {
                evidence: Box::new(success.clone()),
            },
        ),
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure {
                failure: failure.clone(),
            },
        ),
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
        ),
    ];
    for observation in execute_outcomes {
        assert_response_round_trip(&execute, observation);
    }

    let inspect_outcomes = [
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Satisfied {
                evidence: Box::new(success),
            },
        ),
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::NotCompleted {
                evidence: WorkloadOwnerEvidenceDigest::sha256("not-completed"),
            },
        ),
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { failure },
        ),
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::InProgress {
                evidence: WorkloadOwnerEvidenceDigest::sha256("in-progress"),
            },
        ),
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Ambiguous,
        ),
    ];
    for observation in inspect_outcomes {
        assert_response_round_trip(&inspect, observation);
    }
}

#[test]
fn teardown_response_rejects_cross_mode_success_and_every_echoed_fence() {
    let request = request_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    assert_eq!(
        MachineApiWorkloadTeardownPhaseResult::new(
            request.command(),
            MachineApiWorkloadTeardownObservation::Inspect(
                MachineApiWorkloadTeardownInspectObservation::Ambiguous,
            ),
            None,
        ),
        Err(MachineApiWorkloadTeardownWireError::ObservationModeMismatch)
    );
    let crossed_success = success_evidence(
        WorkloadTeardownStep::StopExecution,
        request.command().subjects(),
        "crossed-success",
    );
    assert_eq!(
        MachineApiWorkloadTeardownPhaseResult::new(
            request.command(),
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Succeeded {
                    evidence: Box::new(crossed_success),
                },
            ),
            None,
        ),
        Err(MachineApiWorkloadTeardownWireError::SuccessEvidenceMismatch)
    );

    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(
        &request,
        phase_result(
            &request,
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
            ),
        ),
    )
    .unwrap();
    let value = serde_json::to_value(response).unwrap();
    let other = request_fixture(
        'b',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    let other_response = MachineApiWorkloadTeardownPhaseResponse::for_request(
        &other,
        phase_result(
            &other,
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
            ),
        ),
    )
    .unwrap();
    let other_value = serde_json::to_value(other_response).unwrap();

    for pointer in [
        "/requestDigest",
        "/forwarderAuthority",
        "/commandId",
        "/issuingTransitionId",
        "/confirmedTransitionId",
        "/attemptId",
        "/providerTarget",
        "/subjects",
    ] {
        let mut crossed = value.clone();
        *crossed.pointer_mut(pointer).unwrap() = other_value.pointer(pointer).unwrap().clone();
        let decoded: MachineApiWorkloadTeardownPhaseResponse =
            serde_json::from_value(crossed).unwrap();
        assert!(
            decoded.validate_for_request(&request).is_err(),
            "crossed response fence {pointer} must fail validation"
        );
    }
    for (field, replacement) in [
        ("issuingRevision", json!("999")),
        ("confirmedRevision", json!("999")),
        ("dispatchEpoch", json!("1")),
        ("providerTranslation", json!("guest_container_attachment")),
        ("step", json!("stop_execution")),
        ("mode", json!("inspect")),
    ] {
        let mut crossed = value.clone();
        crossed[field] = replacement;
        let decoded: MachineApiWorkloadTeardownPhaseResponse =
            serde_json::from_value(crossed).unwrap();
        assert!(decoded.validate_for_request(&request).is_err());
    }

    let mut unknown = value;
    unknown["unknown"] = json!(true);
    assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseResponse>(unknown).is_err());
}

#[test]
fn release_success_requires_independent_absence_and_other_outcomes_forbid_it() {
    let release = request_fixture(
        'a',
        WorkloadTeardownStep::ReleaseNetwork,
        WorkloadTeardownCommandMode::Execute,
    );
    let success = MachineApiWorkloadTeardownObservation::Execute(
        MachineApiWorkloadTeardownExecuteObservation::Succeeded {
            evidence: Box::new(success_evidence(
                WorkloadTeardownStep::ReleaseNetwork,
                release.command().subjects(),
                "release-success",
            )),
        },
    );
    assert_eq!(
        MachineApiWorkloadTeardownPhaseResult::new(release.command(), success.clone(), None),
        Err(MachineApiWorkloadTeardownWireError::ReleaseAbsenceEvidenceMismatch)
    );

    let absence = MachineApiNetworkReleaseAbsenceEvidence::new(
        WorkloadOwnerEvidenceDigest::sha256("provider-absent"),
        WorkloadOwnerEvidenceDigest::sha256("publication-absent"),
    );
    let drain = request_fixture(
        'a',
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownCommandMode::Execute,
    );
    assert_eq!(
        MachineApiWorkloadTeardownPhaseResult::new(
            drain.command(),
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
            ),
            Some(absence),
        ),
        Err(MachineApiWorkloadTeardownWireError::ReleaseAbsenceEvidenceMismatch)
    );

    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(
        &release,
        MachineApiWorkloadTeardownPhaseResult::new(release.command(), success, Some(absence))
            .unwrap(),
    )
    .unwrap();
    let mut missing = serde_json::to_value(&response).unwrap();
    missing.as_object_mut().unwrap().remove("releaseAbsence");
    assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseResponse>(missing).is_err());
    let mut null = serde_json::to_value(response).unwrap();
    null["releaseAbsence"] = Value::Null;
    assert!(serde_json::from_value::<MachineApiWorkloadTeardownPhaseResponse>(null).is_err());
}

fn assert_response_round_trip(
    request: &MachineApiWorkloadTeardownPhaseRequest,
    observation: MachineApiWorkloadTeardownObservation,
) {
    let response = MachineApiWorkloadTeardownPhaseResponse::for_request(
        request,
        phase_result(request, observation),
    )
    .unwrap();
    let encoded = serde_json::to_value(&response).unwrap();
    let decoded: MachineApiWorkloadTeardownPhaseResponse = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, response);
    decoded.validate_for_request(request).unwrap();
}

fn phase_result(
    request: &MachineApiWorkloadTeardownPhaseRequest,
    observation: MachineApiWorkloadTeardownObservation,
) -> MachineApiWorkloadTeardownPhaseResult {
    let successful_release = request.command().step() == WorkloadTeardownStep::ReleaseNetwork
        && matches!(
            &observation,
            MachineApiWorkloadTeardownObservation::Execute(
                MachineApiWorkloadTeardownExecuteObservation::Succeeded { .. }
            ) | MachineApiWorkloadTeardownObservation::Inspect(
                MachineApiWorkloadTeardownInspectObservation::Satisfied { .. }
            )
        );
    let release_absence = successful_release.then(|| {
        MachineApiNetworkReleaseAbsenceEvidence::new(
            WorkloadOwnerEvidenceDigest::sha256("provider-absent"),
            WorkloadOwnerEvidenceDigest::sha256("publication-absent"),
        )
    });
    MachineApiWorkloadTeardownPhaseResult::new(request.command(), observation, release_absence)
        .unwrap()
}

fn guest_steps() -> [WorkloadTeardownStep; 4] {
    [
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ]
}

fn request_fixture(
    seed: char,
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
) -> MachineApiWorkloadTeardownPhaseRequest {
    let input = command_input_fixture(seed, step, mode);
    let authority = input.machine_forwarder_authority.clone();
    let command = MachineApiWorkloadTeardownCommandEnvelope::new(input).unwrap();
    MachineApiWorkloadTeardownPhaseRequest::new(authority, command).unwrap()
}

fn command_input_fixture(
    seed: char,
    step: WorkloadTeardownStep,
    mode: WorkloadTeardownCommandMode,
) -> MachineApiWorkloadTeardownCommandEnvelopeInput {
    let fixture = TeardownFixture::new(seed);
    let claim = fixture.claim(step);
    let confirmed_revision = match mode {
        WorkloadTeardownCommandMode::Execute => claim.claimed_revision(),
        WorkloadTeardownCommandMode::Inspect => claim.claimed_revision().checked_next().unwrap(),
    };
    let confirmed_transition_id = fixture.transition(step_index(step) * 2 + 1);
    let command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
        &claim,
        confirmed_revision,
        &confirmed_transition_id,
        mode,
    )
    .unwrap();
    let machine_provider_generation = NetworkResourceGeneration::new(7);
    let machine_forwarder_authority = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(&format!("machine-forwarder-{seed}")),
            format!("machine-forwarder-instance-{seed}"),
        )
        .unwrap(),
        machine_provider_generation,
    );
    let provider_translation = match step {
        WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
            MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
        }
        WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
            MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
        }
        WorkloadTeardownStep::WithdrawPublication => {
            MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
        }
    };
    MachineApiWorkloadTeardownCommandEnvelopeInput {
        command_id,
        confirmed_revision,
        confirmed_transition_id,
        source: fixture.intent.source().clone(),
        compiled_network_plan: fixture.intent.network().compiled_plan().clone(),
        execution_locator: fixture.execution.clone(),
        prior_receipt_prefix: fixture.prefix(step),
        mode,
        claim,
        machine_forwarder_authority,
        machine_provider_generation,
        provider_translation,
    }
}

struct TeardownFixture {
    intent: WorkloadSagaIntent,
    key: WorkloadSagaKey,
    execution: WorkloadExecutionReference,
    network: WorkloadNetworkReference,
    publication: WorkloadPublicationReference,
    cause: WorkloadTeardownCause,
    seed: char,
}

impl TeardownFixture {
    fn new(seed: char) -> Self {
        let tenant_id = TenantId::new(format!("tenant-teardown-{seed}")).unwrap();
        let generation = WorkloadGeneration::new(1);
        let attachment_provider =
            NetworkProviderId::for_registration_key(&format!("attachment-{seed}"));
        let ingress_provider = NetworkProviderId::for_registration_key(&format!("ingress-{seed}"));
        let selection =
            NetworkCapabilitySelection::new(attachment_provider.clone(), ingress_provider.clone());
        let selection_evidence = NetworkCapabilityBundle::new(
            NetworkAttachmentProviderRegistration::new(
                attachment_provider.clone(),
                NetworkAttachmentCapabilitySet::new(
                    NetworkManagementMode::NimbusHostManaged,
                    [],
                    [],
                ),
                [],
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
            NetworkIngressProviderRegistration::new(
                ingress_provider,
                NetworkEndpointCapabilitySet::new([], [], [], [], []),
                NetworkIngressCapabilitySet::new([]),
                NetworkForwardingCapabilitySet::new([]),
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
        )
        .selection_evidence();
        let identity = WorkloadNetworkPlanIdentity::new(
            tenant_id.clone(),
            format!("workload-incarnation-{seed}"),
            NetworkResourceGeneration::new(generation.as_u64()),
        )
        .unwrap();
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
            identity.clone(),
            requirements,
            Some(selection),
            Some(selection_evidence),
            Some(WorkloadNetworkAttachmentBlueprint::new(&identity, "default").unwrap()),
            [],
            [],
            [],
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
        .unwrap();
        let compiled = CompiledWorkloadNetworkPlan::from_content(content).unwrap();
        let executable = WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            format!(r#"{{"fixture":"machine-teardown-{seed}"}}"#),
        )
        .unwrap();
        let execution_provider =
            WorkloadExecutionProviderId::for_registration_key(&format!("execution-{seed}"));
        let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
            WorkloadProvisionSourceIdentity::standalone_sandbox(
                format!("workload-teardown-{seed}"),
                format!("profile-teardown-{seed}"),
            )
            .unwrap(),
            WorkloadProvisionSourceGeneration::new(1),
            WorkloadProvisionSourceResourceVersion::new(format!("version-{seed}")).unwrap(),
            executable.content_digest(),
            attachment_provider,
            execution_provider,
        )
        .unwrap();
        let intent = WorkloadSagaIntent::new_without_automatic_restart(
            DesiredWorkloadKind::Sandbox,
            DesiredWorkloadState::Running,
            generation,
            executable,
            source,
            WorkloadNetworkIntent::new(compiled),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
            WorkloadAdmissionEvidence::new(
                format!("tid_{}", seed.to_string().repeat(64))
                    .try_into()
                    .unwrap(),
                format!("twu_{}", seed.to_string().repeat(64))
                    .try_into()
                    .unwrap(),
                NodeIdentity::new(format!("node-teardown-{seed}")).unwrap(),
            ),
        )
        .unwrap();
        let key = WorkloadSagaKey::new(
            tenant_id,
            WorkloadId::new(format!("workload-teardown-{seed}")).unwrap(),
        );
        let execution = WorkloadExecutionReference::for_intent(&intent);
        let network = WorkloadNetworkReference::for_intent(&intent);
        let publication = WorkloadPublicationReference::new(
            [PublishedEndpointId::for_workload_endpoint(
                &format!("workload-incarnation-{seed}"),
                "api",
            )],
            &intent,
        )
        .unwrap();
        let cause = WorkloadTeardownCause::Successor {
            generation: WorkloadGeneration::new(2),
            desired_digest: WorkloadDesiredDigest::sha256(format!("successor-{seed}")),
        };
        Self {
            intent,
            key,
            execution,
            network,
            publication,
            cause,
            seed,
        }
    }

    fn transition(&self, ordinal: usize) -> WorkloadSagaTransitionId {
        let chars = ['a', 'b', 'c', 'd', 'e', 'f'];
        let value = chars[(step_index_from_seed(self.seed) + ordinal) % chars.len()];
        format!("wst_{}", value.to_string().repeat(64))
            .try_into()
            .unwrap()
    }

    fn attempt(&self, step: WorkloadTeardownStep) -> WorkloadTeardownAttempt {
        let subjects = match step {
            WorkloadTeardownStep::WithdrawPublication => {
                WorkloadTeardownSubjects::Publication(self.publication.clone())
            }
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
                WorkloadTeardownSubjects::Execution(self.execution.clone())
            }
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
                WorkloadTeardownSubjects::Network(self.network.clone())
            }
        };
        self.attempt_with_subjects(step, subjects)
    }

    fn attempt_with_subjects(
        &self,
        step: WorkloadTeardownStep,
        subjects: WorkloadTeardownSubjects,
    ) -> WorkloadTeardownAttempt {
        let index = step_index(step);
        let (source_phase, target_phase) = step.phases();
        WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
            key: self.key.clone(),
            saga_id: self.key.saga_id(),
            issuing_revision: WorkloadSagaRevision::new((index * 2 + 1) as u64),
            issuing_transition_id: self.transition(index * 2),
            generation: self.intent.generation(),
            desired_digest: self.intent.desired_digest(),
            required_node: self.intent.admission().assigned_node().clone(),
            source_digest: self.intent.source().source_digest(),
            execution_provider_id: self.intent.source().execution_provider_id().clone(),
            network_plan_digest: self.intent.network().digest(),
            selection_evidence: self
                .intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .cloned(),
            cause: self.cause.clone(),
            successor_fence: None,
            source_phase,
            target_phase,
            step,
            subjects,
        })
        .unwrap()
    }

    fn claim(&self, step: WorkloadTeardownStep) -> WorkloadTeardownClaim {
        let attempt = self.attempt(step);
        let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
            .unwrap()
            .unwrap();
        serde_json::from_value(json!({
            "attempt": attempt,
            "claimedRevision": attempt.issuing_revision().checked_next().unwrap(),
            "dispatchEpoch": "0",
            "providerTarget": provider_target,
            "authorization": { "kind": "initial" },
        }))
        .unwrap()
    }

    fn receipt(&self, step: WorkloadTeardownStep) -> WorkloadTeardownReceipt {
        let claim = self.claim(step);
        let evidence = success_evidence(step, claim.attempt().subjects(), "prior-receipt");
        serde_json::from_value(json!({
            "claim": claim,
            "evidence": evidence,
            "confirmation": WorkloadTeardownResultConfirmation::Dispatch,
        }))
        .unwrap()
    }

    fn receipt_with_subjects(
        &self,
        step: WorkloadTeardownStep,
        subjects: WorkloadTeardownSubjects,
    ) -> WorkloadTeardownReceipt {
        let attempt = self.attempt_with_subjects(step, subjects);
        let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
            .unwrap()
            .unwrap();
        let claim: WorkloadTeardownClaim = serde_json::from_value(json!({
            "attempt": attempt,
            "claimedRevision": attempt.issuing_revision().checked_next().unwrap(),
            "dispatchEpoch": "0",
            "providerTarget": provider_target,
            "authorization": { "kind": "initial" },
        }))
        .unwrap();
        let evidence = success_evidence(step, claim.attempt().subjects(), "crossed-receipt");
        serde_json::from_value(json!({
            "claim": claim,
            "evidence": evidence,
            "confirmation": WorkloadTeardownResultConfirmation::Dispatch,
        }))
        .unwrap()
    }

    fn prefix(&self, current: WorkloadTeardownStep) -> WorkloadTeardownReceiptPrefix {
        let receipts: Vec<_> = all_steps()
            .into_iter()
            .take(step_index(current))
            .map(|step| self.receipt(step))
            .collect();
        serde_json::from_value(json!({ "receipts": receipts })).unwrap()
    }
}

fn all_steps() -> [WorkloadTeardownStep; 5] {
    [
        WorkloadTeardownStep::WithdrawPublication,
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ]
}

fn step_index(step: WorkloadTeardownStep) -> usize {
    all_steps()
        .iter()
        .position(|candidate| *candidate == step)
        .unwrap()
}

fn step_index_from_seed(seed: char) -> usize {
    match seed {
        'a' => 0,
        'b' => 1,
        'c' => 2,
        'd' => 3,
        'e' => 4,
        'f' => 5,
        _ => panic!("fixture seed must be hexadecimal"),
    }
}

fn success_evidence(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
    label: &str,
) -> WorkloadTeardownSuccessEvidence {
    let digest = WorkloadOwnerEvidenceDigest::sha256(format!("{label}-{step:?}"));
    match (step, subjects) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence: digest,
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence: digest,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence: digest,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence: digest,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence: digest,
            }
        }
        _ => panic!("fixture step must match its typed subjects"),
    }
}
