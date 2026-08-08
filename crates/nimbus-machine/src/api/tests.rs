#[cfg(unix)]
use nimbus_core::WorkloadId;
#[cfg(unix)]
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkProviderHandle, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyRequirements,
};
#[cfg(unix)]
use nimbus_sandbox::{
    MachinePortForwardOutcome, SandboxCleanupObservation, SandboxExecutionObservation,
    SandboxHandle, SandboxOwnerSpec, SandboxProcessSpec, SandboxRestartAssessment,
    SandboxRestartBlocker, SandboxRootSpec, SandboxSpec,
};
#[cfg(unix)]
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, NodeIdentity, TenantWorkloadUid, WorkloadActivationIntent,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadExecutionId,
    WorkloadExecutionProviderId, WorkloadExecutionReference, WorkloadGeneration,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadProvisionAttempt,
    WorkloadProvisionAttemptInput, WorkloadProvisionDispatchClaim, WorkloadProvisionProviderTarget,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadProvisionSubjects, WorkloadPublicationIntent, WorkloadSagaKey, WorkloadSagaPhase,
    WorkloadSagaRevision, WorkloadSagaTransitionId,
};

use super::*;

#[test]
fn machine_api_query_path_percent_encodes_query_delimiters() {
    let path = machine_api_query_path(
        MACHINE_API_CURRENT_SERVICE_SANDBOX_PATH,
        &[
            ("tenant_id", "tenant"),
            ("service_name", "db & cache=1/path☁"),
        ],
    );

    assert_eq!(
        path,
        "/v1/machine-api/service-sandboxes/current?tenant_id=tenant&service_name=db%20%26%20cache%3D1%2Fpath%E2%98%81"
    );
}

#[test]
fn machine_api_path_segment_encodes_reserved_and_structural_characters() {
    assert_eq!(machine_api_path_segment("db-1"), "db-1");
    assert_eq!(machine_api_path_segment("../etc"), "..%2Fetc");
    assert_eq!(machine_api_path_segment("a/b"), "a%2Fb");
    assert_eq!(machine_api_path_segment("a b"), "a%20b");
    assert_eq!(machine_api_path_segment("50%off"), "50%25off");
    assert_eq!(machine_api_path_segment("q?x#y"), "q%3Fx%23y");
}

#[test]
fn machine_api_service_sandbox_paths_use_encoded_single_segments() {
    assert_eq!(
        machine_api_service_sandbox_path("x/y"),
        "/v1/machine-api/service-sandboxes/x%2Fy"
    );
    assert_eq!(
        machine_api_service_sandbox_logs_path("x/y", 7),
        "/v1/machine-api/service-sandboxes/x%2Fy/logs?offset=7"
    );
    assert_eq!(
        machine_api_service_sandbox_process_snapshot_path("p#q"),
        "/v1/machine-api/service-sandboxes/p%23q/ps"
    );
    assert_eq!(
        machine_api_service_sandbox_stop_path("a b%c"),
        "/v1/machine-api/service-sandboxes/a%20b%25c/stop"
    );
}

#[cfg(unix)]
#[test]
fn service_sandbox_retirement_dtos_are_strict_and_preserve_exact_evidence() {
    let sandbox_id = SandboxId::new("sandbox-machine-api-01");
    let authority = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("machine-gvproxy"),
            "machine-config-01",
        )
        .expect("provider fixture should validate"),
        NetworkResourceGeneration::new(11),
    );
    let bindings = vec![
        SandboxPortBinding::tcp("http", 18_080, 8_080),
        SandboxPortBinding::tcp("metrics", 19_090, 9_090),
    ];
    let spec = SandboxSpec::new(
        TenantId::new("tenant-machine-api").expect("tenant fixture should validate"),
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/tmp/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_bindings(bindings.clone());

    let stop_request = MachineApiServiceSandboxStopRequest {
        forwarder_authority: authority.clone(),
    };
    assert_strict_authority_request(&stop_request, "stop request");
    assert_strict_authority_request(
        &MachineApiBootcSwitchRequest {
            forwarder_authority: authority.clone(),
            image: "ghcr.io/nimbus/machine-os:next".to_owned(),
            transport: Some("registry".to_owned()),
        },
        "bootc switch request",
    );
    assert_strict_authority_request(
        &MachineApiBootcUpgradeRequest {
            forwarder_authority: authority.clone(),
            check: false,
            tag: None,
        },
        "bootc upgrade request",
    );
    assert_strict_authority_request(
        &MachineApiBootcRollbackRequest {
            forwarder_authority: authority.clone(),
        },
        "bootc rollback request",
    );

    let handle = SandboxHandle::new(
        spec.tenant_id.clone(),
        sandbox_id.clone(),
        "api",
        SandboxBackendKind::Container,
        SandboxStatus::Ready,
        Vec::new(),
    );
    let inspection = SandboxInspection::provider_reported(handle.clone()).with_provider_projection(
        handle.clone(),
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
            completed_restarts: 1,
            retry_delay_millis: 2_000,
            persisted_not_before_millis: Some(9_000),
            blocker: Some(SandboxRestartBlocker::StartupReconciliationUnavailable),
        },
        SandboxCleanupObservation::Retained,
    );
    let inspect_response = MachineApiServiceSandboxInspectResponse {
        sandbox_id: sandbox_id.clone(),
        inspection: Some(inspection.clone()),
    };
    let inspect_value =
        serde_json::to_value(&inspect_response).expect("inspection response should serialize");
    assert_eq!(
        serde_json::from_value::<MachineApiServiceSandboxInspectResponse>(inspect_value.clone())
            .expect("inspection response should deserialize"),
        inspect_response,
        "every typed inspection field and exact version must round trip"
    );
    assert_eq!(
        inspect_response
            .inspection
            .as_ref()
            .expect("inspection should remain present")
            .version,
        inspection.version
    );
    assert_unknown_field_rejected::<MachineApiServiceSandboxInspectResponse>(
        inspect_value,
        "inspection response",
    );
    let absent = bindings
        .iter()
        .map(|binding| MachinePortForwardReceipt {
            outcome: MachinePortForwardOutcome::ExactAlreadyAbsent,
            tenant_id: spec.tenant_id.clone(),
            sandbox_id: sandbox_id.clone(),
            binding: binding.clone(),
            provider_instance: authority.provider_instance().clone(),
            provider_generation: authority.generation(),
        })
        .collect::<Vec<_>>();
    let stop = MachineApiServiceSandboxStopResponse {
        tenant_id: spec.tenant_id.clone(),
        sandbox_id: SandboxId::new("sandbox-machine-api-01"),
        stopped: true,
        forwarder_authority: authority.clone(),
        confirmed_absent_evidence: absent.clone(),
    };
    let stop_value = serde_json::to_value(&stop).expect("stop response should serialize");
    assert_eq!(
        serde_json::from_value::<MachineApiServiceSandboxStopResponse>(stop_value.clone())
            .expect("stop response should deserialize"),
        stop
    );
    assert_eq!(stop.forwarder_authority, authority);
    assert_eq!(stop.confirmed_absent_evidence, absent);
    assert_unknown_field_rejected::<MachineApiServiceSandboxStopResponse>(
        stop_value,
        "stop response",
    );
    let mut stale_stop = serde_json::to_value(&stop).expect("stale stop fixture should serialize");
    stale_stop["confirmed_absent_evidence"][0]["provider_generation"] =
        serde_json::json!(authority.generation().as_u64() + 1);
    assert!(
        serde_json::from_value::<MachineApiServiceSandboxStopResponse>(stale_stop).is_err(),
        "the strict response DTO must reject stale stop provider generations"
    );
    let mut duplicate_stop =
        serde_json::to_value(&stop).expect("duplicate stop fixture should serialize");
    duplicate_stop["confirmed_absent_evidence"][1] =
        duplicate_stop["confirmed_absent_evidence"][0].clone();
    assert!(
        serde_json::from_value::<MachineApiServiceSandboxStopResponse>(duplicate_stop).is_err(),
        "the strict response DTO must reject duplicate absence evidence that substitutes for \
         an omitted member"
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_round_trips_strictly() {
    let request = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let request_value = serde_json::to_value(&request).expect("request should serialize");
    assert_eq!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(request_value)
            .expect("request should deserialize"),
        request
    );

    let response = MachineApiWorkloadProvisionPhaseResponse::for_request(
        &request,
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"guest-owner-evidence".to_vec(),
        },
    )
    .expect("response should correlate");
    let response_value = serde_json::to_value(&response).expect("response should serialize");
    let round_trip =
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(response_value)
            .expect("response should deserialize");
    round_trip
        .validate_for_request(&request)
        .expect("response should retain every command fence");
    assert_eq!(round_trip, response);
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_accepts_later_confirmed_inspection_revision() {
    let initial = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let initial_command = initial.command();
    let later_revision = initial_command
        .confirmed_revision()
        .checked_next()
        .expect("fixture revision should advance");
    let transition_id: WorkloadSagaTransitionId = format!("wst_{}", "d".repeat(64))
        .try_into()
        .expect("later transition should validate");
    let command_id = WorkloadProvisionCommandId::for_confirmed_dispatch(
        initial_command.claim(),
        later_revision,
        &transition_id,
        initial_command.execution(),
        WorkloadProvisionCommandMode::Inspect,
    )
    .expect("later inspection identity should encode");
    let later_command = MachineApiWorkloadProvisionCommandEnvelope::new(
        command_id,
        initial_command.attempt_id().clone(),
        initial_command.dispatch_epoch(),
        initial_command.provider_target().clone(),
        initial_command.claim().clone(),
        later_revision,
        transition_id,
        initial_command.generation(),
        initial_command.desired_digest(),
        initial_command.source().clone(),
        initial_command.network_plan_digest(),
        initial_command.execution().clone(),
        initial_command.executable().clone(),
        initial_command.compiled_network_plan().clone(),
        initial_command.machine_provider_generation(),
        WorkloadProvisionCommandMode::Inspect,
    )
    .expect("a later confirmed revision may inspect the retained exact claim");
    let later = MachineApiWorkloadProvisionPhaseRequest::new(
        initial.forwarder_authority().clone(),
        later_command,
    )
    .expect("later inspection request should retain machine authority");

    let encoded = serde_json::to_value(&later).expect("later inspection should serialize");
    assert_eq!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(encoded)
            .expect("later inspection should deserialize"),
        later
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_rejects_execute_at_later_confirmed_revision() {
    let initial = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let initial_command = initial.command();
    let later_revision = initial_command
        .confirmed_revision()
        .checked_next()
        .expect("fixture revision should advance");
    let transition_id: WorkloadSagaTransitionId = format!("wst_{}", "e".repeat(64))
        .try_into()
        .expect("later transition should validate");
    let command_id = WorkloadProvisionCommandId::for_confirmed_dispatch(
        initial_command.claim(),
        later_revision,
        &transition_id,
        initial_command.execution(),
        WorkloadProvisionCommandMode::Execute,
    )
    .expect("later execute identity should encode");

    let result = MachineApiWorkloadProvisionCommandEnvelope::new(
        command_id,
        initial_command.attempt_id().clone(),
        initial_command.dispatch_epoch(),
        initial_command.provider_target().clone(),
        initial_command.claim().clone(),
        later_revision,
        transition_id,
        initial_command.generation(),
        initial_command.desired_digest(),
        initial_command.source().clone(),
        initial_command.network_plan_digest(),
        initial_command.execution().clone(),
        initial_command.executable().clone(),
        initial_command.compiled_network_plan().clone(),
        initial_command.machine_provider_generation(),
        WorkloadProvisionCommandMode::Execute,
    );

    assert_eq!(
        result,
        Err(MachineApiWorkloadProvisionWireError::ConfirmedRevisionMismatch)
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_rejects_unknown_fields_and_malformed_command_ids() {
    let request = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let value = serde_json::to_value(&request).expect("request should serialize");

    let mut root_unknown = value.clone();
    root_unknown["unexpected"] = serde_json::json!(true);
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(root_unknown).is_err()
    );

    let mut command_unknown = value.clone();
    command_unknown["command"]["unexpected"] = serde_json::json!(true);
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(command_unknown).is_err()
    );

    for malformed in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
        let mut candidate = value.clone();
        candidate["command"]["command_id"] = serde_json::json!(malformed);
        assert!(
            serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(candidate).is_err()
        );
    }
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_rejects_crossed_command_fields() {
    let request = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let other = provision_request_fixture(
        'b',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let value = serde_json::to_value(&request).expect("request should serialize");
    let other_value = serde_json::to_value(&other).expect("other request should serialize");

    assert_crossed_request_rejected(&value, "/command/attempt_id", &other_value);
    assert_crossed_request_rejected(&value, "/command/execution", &other_value);
    assert_crossed_request_rejected(
        &value,
        "/command/generation",
        &serde_json::json!({ "command": { "generation": "2" } }),
    );
    assert_crossed_request_rejected(&value, "/command/desired_digest", &other_value);
    assert_crossed_request_rejected(&value, "/command/source", &other_value);
    assert_crossed_request_rejected(&value, "/command/executable", &other_value);
    assert_crossed_request_rejected(&value, "/command/network_plan_digest", &other_value);
    assert_crossed_request_rejected(&value, "/command/transition_id", &other_value);
    assert_crossed_request_rejected(
        &value,
        "/command/confirmed_revision",
        &serde_json::json!({
            "command": { "confirmed_revision": "2" }
        }),
    );

    let mut crossed_provider_generation = value.clone();
    crossed_provider_generation["command"]["machine_provider_generation"] = serde_json::json!(8);
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(
            crossed_provider_generation
        )
        .is_err()
    );

    let mut crossed_tenant = other_value;
    crossed_tenant["command"]["compiled_network_plan"] =
        value["command"]["compiled_network_plan"].clone();
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(crossed_tenant).is_err()
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_authenticates_executable_from_durable_source_preimage() {
    let request = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let other = provision_request_fixture(
        'b',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let mut substituted = serde_json::to_value(request).expect("request should serialize");
    let other = serde_json::to_value(other).expect("other request should serialize");

    substituted["command"]["executable"] = other["command"]["executable"].clone();
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(substituted).is_err(),
        "a self-consistent replacement executable and content digest must not authenticate against the retained source preimage"
    );

    let mut substituted_source = serde_json::to_value(provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    ))
    .expect("request should serialize");
    substituted_source["command"]["executable"] = other["command"]["executable"].clone();
    substituted_source["command"]["source"] = other["command"]["source"].clone();
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(substituted_source)
            .is_err(),
        "replacement executable and recomputed source evidence must remain crossed with the durable claim"
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_wire_enforces_inspect_execute_legality() {
    let inspect = provision_request_fixture(
        'c',
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadProvisionCommandMode::Inspect,
    );
    let inspect_value = serde_json::to_value(&inspect).expect("inspect request should serialize");
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(inspect_value.clone())
            .is_ok()
    );

    let mut illegal_execute = inspect_value;
    illegal_execute["command"]["mode"] = serde_json::json!("execute");
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(illegal_execute).is_err()
    );
}

#[cfg(unix)]
#[test]
fn workload_provision_phase_response_correlates_fences_and_closed_observations() {
    let request = provision_request_fixture(
        'a',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let other = provision_request_fixture(
        'b',
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadProvisionCommandMode::Execute,
    );
    let observations = [
        MachineApiWorkloadProvisionObservation::Succeeded { evidence: vec![1] },
        MachineApiWorkloadProvisionObservation::DefiniteFailure { evidence: vec![2] },
        MachineApiWorkloadProvisionObservation::Absent { evidence: vec![3] },
        MachineApiWorkloadProvisionObservation::InProgress { evidence: vec![4] },
        MachineApiWorkloadProvisionObservation::Ambiguous { evidence: vec![5] },
    ];
    for observation in observations {
        let response = MachineApiWorkloadProvisionPhaseResponse::for_request(&request, observation)
            .expect("closed observation should correlate");
        let value = serde_json::to_value(&response).expect("response should serialize");
        let decoded =
            serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(value.clone())
                .expect("closed observation should deserialize");
        decoded
            .validate_for_request(&request)
            .expect("closed observation should retain exact fences");

        let mut unknown_observation = value;
        unknown_observation["observation"]["unexpected"] = serde_json::json!(true);
        assert!(
            serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(unknown_observation)
                .is_err()
        );
    }
    let mut unknown_kind = serde_json::to_value(
        MachineApiWorkloadProvisionPhaseResponse::for_request(
            &request,
            MachineApiWorkloadProvisionObservation::Ambiguous {
                evidence: Vec::new(),
            },
        )
        .expect("ambiguous response should correlate"),
    )
    .expect("ambiguous response should serialize");
    unknown_kind["observation"]["kind"] = serde_json::json!("future_observation");
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(unknown_kind).is_err()
    );
    assert_eq!(
        MachineApiWorkloadProvisionPhaseResponse::for_request(
            &request,
            MachineApiWorkloadProvisionObservation::InProgress {
                evidence: vec![0; MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES + 1],
            },
        ),
        Err(MachineApiWorkloadProvisionWireError::EvidenceTooLarge {
            size: MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES + 1,
            max: MAX_MACHINE_API_PROVISION_EVIDENCE_BYTES,
        })
    );

    let response = MachineApiWorkloadProvisionPhaseResponse::for_request(
        &request,
        MachineApiWorkloadProvisionObservation::InProgress {
            evidence: b"still-running".to_vec(),
        },
    )
    .expect("response should correlate");
    let value = serde_json::to_value(&response).expect("response should serialize");
    let other_value = serde_json::to_value(&other).expect("other request should serialize");
    let mut crossed_authority = value.clone();
    crossed_authority["forwarder_authority"] = other_value["forwarder_authority"].clone();
    let decoded =
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(crossed_authority)
            .expect("crossed response authority should remain structurally valid");
    assert_eq!(
        decoded.validate_for_request(&request),
        Err(MachineApiWorkloadProvisionWireError::ResponseAuthorityMismatch)
    );
    for (field, expected_error) in [
        (
            "command_id",
            MachineApiWorkloadProvisionWireError::ResponseCommandMismatch,
        ),
        (
            "attempt_id",
            MachineApiWorkloadProvisionWireError::ResponseAttemptMismatch,
        ),
        (
            "provider_target",
            MachineApiWorkloadProvisionWireError::ResponseProviderTargetMismatch,
        ),
    ] {
        let mut crossed = value.clone();
        crossed[field] = other_value["command"][field].clone();
        let decoded = serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(crossed)
            .expect("crossed response fence should remain structurally valid");
        assert_eq!(decoded.validate_for_request(&request), Err(expected_error));
    }
    let mut crossed_epoch = value;
    crossed_epoch["dispatch_epoch"] = serde_json::json!("1");
    let decoded = serde_json::from_value::<MachineApiWorkloadProvisionPhaseResponse>(crossed_epoch)
        .expect("crossed epoch should remain structurally valid");
    assert_eq!(
        decoded.validate_for_request(&request),
        Err(MachineApiWorkloadProvisionWireError::ResponseEpochMismatch)
    );
}

#[cfg(unix)]
fn provision_request_fixture(
    hex: char,
    step: WorkloadProvisionStep,
    mode: WorkloadProvisionCommandMode,
) -> MachineApiWorkloadProvisionPhaseRequest {
    let tenant_id =
        TenantId::new(format!("tenant-wire-{hex}")).expect("fixture tenant should validate");
    let generation = WorkloadGeneration::new(1);
    let desired_digest = WorkloadDesiredDigest::sha256(format!("desired-{hex}"));
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"machine-wire-{hex}"}}"#),
    )
    .expect("fixture executable should validate");
    let execution_provider =
        WorkloadExecutionProviderId::for_registration_key(&format!("machine-execution-{hex}"));
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            format!("workload-wire-{hex}"),
            format!("profile-wire-{hex}"),
        )
        .expect("fixture source identity should validate"),
        WorkloadProvisionSourceGeneration::new(1),
        WorkloadProvisionSourceResourceVersion::new(format!("source-version-{hex}"))
            .expect("fixture source version should validate"),
        executable.content_digest(),
        NetworkProviderId::for_registration_key(&format!("machine-attachment-{hex}")),
        execution_provider.clone(),
    )
    .expect("fixture source evidence should validate");
    let source_digest = source.source_digest();
    let node_identity =
        NodeIdentity::new(format!("node-wire-{hex}")).expect("fixture node should validate");
    let workload_uid: TenantWorkloadUid = format!("twu_{}", hex.to_string().repeat(64))
        .try_into()
        .expect("fixture workload uid should validate");
    let execution_id =
        WorkloadExecutionId::for_execution(&workload_uid, &node_identity, generation);
    let execution: WorkloadExecutionReference = serde_json::from_value(serde_json::json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "generation": generation,
        "desiredDigest": desired_digest,
    }))
    .expect("fixture execution should validate");
    let compiled_network_plan = compiled_network_plan_fixture(&tenant_id, hex, generation);
    let network_plan_digest = compiled_network_plan.plan().digest();
    let network_reference = serde_json::from_value(serde_json::json!({
        "planId": compiled_network_plan.plan().plan_id(),
        "generation": generation.to_string(),
        "digest": network_plan_digest,
    }))
    .expect("fixture network reference should validate");
    let subjects = match step {
        WorkloadProvisionStep::PrepareWorkload => {
            WorkloadProvisionSubjects::Execution(execution.clone())
        }
        WorkloadProvisionStep::InspectActivationPrerequisites => {
            WorkloadProvisionSubjects::Readiness {
                network: network_reference,
                execution: execution.clone(),
            }
        }
        _ => panic!("fixture supports only prepare and prerequisite inspection"),
    };
    let (source_phase, target_phase) = match step {
        WorkloadProvisionStep::PrepareWorkload => (
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared,
        ),
        WorkloadProvisionStep::InspectActivationPrerequisites => (
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::NetworkAttached,
        ),
        _ => unreachable!(),
    };
    let key = WorkloadSagaKey::new(
        tenant_id,
        WorkloadId::new(format!("workload-wire-{hex}"))
            .expect("fixture workload id should validate"),
    );
    let attempt = WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        saga_id: key.saga_id(),
        key,
        issuing_revision: WorkloadSagaRevision::new(0),
        generation,
        desired_digest,
        required_node: node_identity,
        source_digest,
        execution_provider_id: execution_provider.clone(),
        network_plan_digest,
        selection_evidence: None,
        source_phase,
        target_phase,
        step,
        subjects,
        prerequisite: None,
    })
    .expect("fixture attempt should validate");
    let provider_target = WorkloadProvisionProviderTarget::Execution {
        provider_id: execution_provider,
        provider_source_digest: source_digest,
    };
    let claim: WorkloadProvisionDispatchClaim = serde_json::from_value(serde_json::json!({
        "attempt": attempt,
        "claimedRevision": "1",
        "dispatchEpoch": "0",
        "providerTarget": provider_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("fixture dispatch claim should validate");
    let transition_id: WorkloadSagaTransitionId = format!("wst_{}", hex.to_string().repeat(64))
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
    .expect("fixture command identity should encode");
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
        network_plan_digest,
        execution,
        executable,
        compiled_network_plan,
        NetworkResourceGeneration::new(7),
        mode,
    )
    .expect("fixture command should validate");
    let authority = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(&format!("machine-forwarder-{hex}")),
            format!("machine-instance-{hex}"),
        )
        .expect("fixture provider handle should validate"),
        NetworkResourceGeneration::new(7),
    );
    MachineApiWorkloadProvisionPhaseRequest::new(authority, command)
        .expect("fixture request should validate")
}

#[cfg(unix)]
fn compiled_network_plan_fixture(
    tenant_id: &TenantId,
    hex: char,
    generation: WorkloadGeneration,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("workload-incarnation-{hex}"),
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
        .expect("fixture compiled network plan should validate")
}

#[cfg(unix)]
fn assert_crossed_request_rejected(
    baseline: &serde_json::Value,
    pointer: &str,
    source: &serde_json::Value,
) {
    let mut crossed = baseline.clone();
    *crossed
        .pointer_mut(pointer)
        .expect("baseline pointer should resolve") = source
        .pointer(pointer)
        .expect("source pointer should resolve")
        .clone();
    assert!(
        serde_json::from_value::<MachineApiWorkloadProvisionPhaseRequest>(crossed).is_err(),
        "crossed field {pointer} must fail closed"
    );
}

#[cfg(unix)]
fn assert_strict_authority_request<T>(request: &T, label: &str)
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let value = serde_json::to_value(request).expect("request should serialize");
    let round_trip =
        serde_json::from_value::<T>(value.clone()).expect("request should deserialize");
    assert_eq!(&round_trip, request, "{label} must round trip exactly");

    let mut missing = value.clone();
    missing
        .as_object_mut()
        .expect("request wire should be an object")
        .remove("forwarder_authority");
    assert!(
        serde_json::from_value::<T>(missing).is_err(),
        "{label} must reject a missing authority"
    );
    assert_unknown_field_rejected::<T>(value, label);
}

#[cfg(unix)]
fn assert_unknown_field_rejected<T>(mut value: serde_json::Value, label: &str)
where
    T: for<'de> Deserialize<'de>,
{
    value
        .as_object_mut()
        .expect("wire should be an object")
        .insert("unexpected".to_owned(), serde_json::json!(true));
    assert!(
        serde_json::from_value::<T>(value).is_err(),
        "{label} must reject unknown fields"
    );
}
