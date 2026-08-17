#[cfg(unix)]
use nimbus_core::WorkloadId;
#[cfg(unix)]
use nimbus_network::{
    EndpointProtocol, NetworkAttachmentCapabilitySet, NetworkAttachmentHandle, NetworkAttachmentId,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyRequirements, PublishedEndpoint, PublishedEndpointHandle,
    PublishedEndpointId,
};
#[cfg(unix)]
use nimbus_sandbox::{
    SandboxCleanupObservation, SandboxExecutionAttemptId, SandboxExecutionObservation,
    SandboxHandle, SandboxNetworkStatus, SandboxOwnerSpec, SandboxProcessSpec,
    SandboxRestartAssessment, SandboxRestartBlocker, SandboxRootSpec, SandboxSpec,
};
#[cfg(unix)]
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    TenantWorkloadUid, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadEffectReferences, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadExecutionAttemptId, WorkloadExecutionId, WorkloadExecutionProviderId,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadInspectionVersion,
    WorkloadNetworkIntent, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadNetworkReference, WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation,
    WorkloadPhaseDetail, WorkloadProvisionAttempt, WorkloadProvisionAttemptInput,
    WorkloadProvisionDispatchClaim, WorkloadProvisionPrerequisiteEvidence,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadPublicationReference,
    WorkloadRestartAdmissionInput, WorkloadRestartAdmissionUpdate, WorkloadRestartCommandClaim,
    WorkloadRestartEpoch, WorkloadRestartEvidenceDigest, WorkloadRestartNotBeforeUnixMillis,
    WorkloadRestartPolicy, WorkloadRestartRequestId, WorkloadRestartTrigger, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaRevision,
    WorkloadSagaTransitionId,
};

use super::*;

// Ownership reason: this 1,500-line test module keeps every strict Machine API
// wire DTO under one private contract suite; production owners stay split by
// concept, and no test fixture becomes a reusable runtime authority.

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
}

#[cfg(unix)]
#[test]
fn service_sandbox_inspection_dto_is_strict_and_preserves_exact_evidence() {
    let sandbox_id = SandboxId::new("sandbox-machine-api-01");
    let authority = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("machine-gvproxy"),
            "machine-config-01",
        )
        .expect("provider fixture should validate"),
        NetworkResourceGeneration::new(11),
    );
    let spec = SandboxSpec::new(
        TenantId::new("tenant-machine-api").expect("tenant fixture should validate"),
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/tmp/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    );
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
    let generation = NetworkResourceGeneration::new(9);
    let endpoint = PublishedEndpoint::new(
        "api",
        EndpointProtocol::Https,
        "127.0.0.1:8443".parse().expect("endpoint should parse"),
    );
    let network_status = SandboxNetworkStatus::new(
        Some(NetworkAttachmentHandle::new(
            NetworkAttachmentId::for_workload_attachment("machine-api/status", "primary"),
            generation,
        )),
        [PublishedEndpointHandle::new(
            PublishedEndpointId::for_workload_endpoint("machine-api/status", "api"),
            generation,
            endpoint,
        )],
    )
    .expect("portable network status should validate");
    let inspection = SandboxInspection::provider_authenticated_running_with_network_status(
        handle.clone(),
        Some(network_status),
        SandboxExecutionAttemptId::new("machine-api-status-attempt")
            .expect("attempt should validate"),
        b"provider-evidence-does-not-enter-wire-fields",
    )
    .with_provider_projection(
        handle.clone(),
        SandboxExecutionObservation::Exited { exit_code: 42 },
        SandboxRestartAssessment::Candidate {
            exit_code: 42,
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
    let inspect_json = inspect_value.to_string();
    assert!(inspect_json.contains("network_status"));
    assert!(inspect_json.contains("attachmentId"));
    assert!(!inspect_json.contains("opaque_value"));
    assert!(!inspect_json.contains("provider-evidence-does-not-enter-wire-fields"));
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
}

#[cfg(unix)]
#[test]
fn service_sandbox_list_and_lookup_round_trip_portable_network_status() {
    let tenant_id = TenantId::new("tenant-machine-summary").expect("tenant should validate");
    let sandbox_id = SandboxId::new("sandbox-machine-summary-01");
    let generation = NetworkResourceGeneration::new(13);
    let attachment_id = NetworkAttachmentId::for_workload_attachment("machine-summary", "primary");
    let endpoint_id = PublishedEndpointId::for_workload_endpoint("machine-summary", "api");
    let endpoint = PublishedEndpoint::new(
        "api",
        EndpointProtocol::Http,
        "127.0.0.1:8080".parse().expect("endpoint should parse"),
    )
    .with_guest_port(8080);
    let network_status = SandboxNetworkStatus::new(
        Some(NetworkAttachmentHandle::new(
            attachment_id.clone(),
            generation,
        )),
        [PublishedEndpointHandle::new(
            endpoint_id.clone(),
            generation,
            endpoint.clone(),
        )],
    )
    .expect("portable status should validate");
    let summary = MachineApiServiceSandboxSummary {
        sandbox_id,
        tenant_id: tenant_id.clone(),
        service_name: "api".to_owned(),
        status: SandboxStatus::Ready,
        published_endpoints: vec![endpoint],
        network_status: Some(network_status),
        last_exit_code: None,
        shutdown_requested: false,
    };
    let list = MachineApiServiceSandboxListResponse {
        sandboxes: vec![summary.clone()],
    };
    let lookup = MachineApiServiceSandboxLookupResponse {
        tenant_id,
        service_name: "api".to_owned(),
        details: Some(MachineApiServiceSandboxDetails {
            summary,
            resources: SandboxResourceLimits::default(),
            lifecycle: SandboxLifecycleSpec::default(),
            port_bindings: Vec::new(),
            log_paths: MachineApiServiceSandboxLogPaths {
                ctr_log: "/state/ctr.log".into(),
                oci_log: "/state/oci.log".into(),
            },
            state_dir: "/state".into(),
            manifest_path: "/state/manifest.json".into(),
        }),
    };

    let list_json = serde_json::to_string(&list).expect("list should serialize");
    let lookup_json = serde_json::to_string(&lookup).expect("lookup should serialize");
    assert_eq!(
        serde_json::from_str::<MachineApiServiceSandboxListResponse>(&list_json)
            .expect("list should deserialize"),
        list
    );
    assert_eq!(
        serde_json::from_str::<MachineApiServiceSandboxLookupResponse>(&lookup_json)
            .expect("lookup should deserialize"),
        lookup
    );
    for json in [&list_json, &lookup_json] {
        assert!(json.contains(attachment_id.as_str()));
        assert!(json.contains(endpoint_id.as_str()));
        assert!(!json.contains("provider"));
        assert!(!json.contains("opaque"));
    }
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
#[test]
fn workload_restart_phase_wire_round_trips_automatic_and_explicit_requests_strictly() {
    let automatic =
        restart_request_fixture('a', true, MachineApiWorkloadRestartCommandMode::Execute);
    let explicit =
        restart_request_fixture('b', false, MachineApiWorkloadRestartCommandMode::Execute);

    for request in [&automatic, &explicit] {
        let value = serde_json::to_value(request).expect("restart request should serialize");
        let round_trip = serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(value)
            .expect("restart request should deserialize");
        assert_eq!(&round_trip, request);
    }
    assert!(automatic.command().inspection_version().is_some());
    assert_eq!(explicit.command().inspection_version(), None);
    assert_ne!(automatic.request_digest(), explicit.request_digest());

    let inspection =
        restart_request_fixture('c', false, MachineApiWorkloadRestartCommandMode::Inspect);
    assert_eq!(
        inspection.command().mode(),
        MachineApiWorkloadRestartCommandMode::Inspect
    );
    assert_eq!(
        inspection.command().confirmed_revision(),
        inspection
            .command()
            .issuing_revision()
            .checked_next()
            .and_then(WorkloadSagaRevision::checked_next)
            .expect("inspection fixture revision should advance twice")
    );

    let mut later_veto =
        serde_json::to_value(inspection.command()).expect("inspection command should serialize");
    let successor_generation = inspection
        .command()
        .generation()
        .checked_next()
        .expect("fixture generation should have a successor");
    let later_revision = inspection
        .command()
        .confirmed_revision()
        .checked_next()
        .expect("fixture inspection revision should advance");
    later_veto["successor_veto_generation"] =
        serde_json::to_value(successor_generation).expect("successor generation should serialize");
    later_veto["confirmed_revision"] =
        serde_json::to_value(later_revision).expect("later revision should serialize");
    let later_veto = serde_json::from_value::<MachineApiWorkloadRestartCommandEnvelope>(later_veto)
        .expect("later successor-veto inspection should authenticate");
    assert_eq!(
        later_veto.successor_veto_generation(),
        Some(successor_generation)
    );

    let later_veto_value =
        serde_json::to_value(&later_veto).expect("successor-veto request should serialize");
    let mut unauthenticated_later_revision = later_veto_value.clone();
    unauthenticated_later_revision["successor_veto_generation"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<MachineApiWorkloadRestartCommandEnvelope>(
            unauthenticated_later_revision
        )
        .is_err(),
        "a later inspection revision without exact veto evidence must fail closed"
    );

    let mut crossed_veto_generation = later_veto_value.clone();
    crossed_veto_generation["successor_veto_generation"] =
        serde_json::to_value(later_veto.generation()).expect("crossed generation should serialize");
    assert!(
        serde_json::from_value::<MachineApiWorkloadRestartCommandEnvelope>(crossed_veto_generation)
            .is_err(),
        "a veto that does not name a later generation must fail closed"
    );

    let mut execute_with_veto = later_veto_value;
    execute_with_veto["mode"] = serde_json::json!("execute");
    execute_with_veto["confirmed_revision"] = serde_json::to_value(
        later_veto
            .issuing_revision()
            .checked_next()
            .expect("execute revision should exist"),
    )
    .expect("execute revision should serialize");
    assert!(
        serde_json::from_value::<MachineApiWorkloadRestartCommandEnvelope>(execute_with_veto)
            .is_err(),
        "execute authority must reject successor-veto evidence"
    );
}

#[cfg(unix)]
#[test]
fn machine_restart_wire_rejects_crossed_fences() {
    let request = restart_request_fixture('a', true, MachineApiWorkloadRestartCommandMode::Execute);
    let other = restart_request_fixture('b', true, MachineApiWorkloadRestartCommandMode::Execute);
    let value = serde_json::to_value(&request).expect("restart request should serialize");
    let other_value = serde_json::to_value(&other).expect("other request should serialize");

    for pointer in [
        "/forwarder_authority",
        "/command/command_id",
        "/command/key",
        "/command/saga_id",
        "/command/transition_id",
        "/command/desired_digest",
        "/command/source",
        "/command/source_execution",
        "/command/execution",
        "/command/source_attempt_id",
        "/command/attempt_id",
        "/command/request_id",
        "/command/inspection_version",
        "/command/provider_selection",
        "/command/claim",
        "/command/executable",
        "/command/network_plan_digest",
        "/command/compiled_network_plan",
        "/command/machine_forwarder_authority",
    ] {
        assert_crossed_restart_request_rejected(&value, pointer, &other_value);
    }
    for (pointer, replacement) in [
        ("/command/generation", serde_json::json!("2")),
        ("/command/restart_epoch", serde_json::json!("2")),
        ("/command/dispatch_epoch", serde_json::json!("1")),
        ("/command/issuing_revision", serde_json::json!("999")),
        ("/command/confirmed_revision", serde_json::json!("999")),
        ("/command/step", serde_json::json!("prepare_execution")),
        ("/command/mode", serde_json::json!("inspect")),
        ("/command/machine_provider_generation", serde_json::json!(8)),
    ] {
        let mut crossed = value.clone();
        *crossed
            .pointer_mut(pointer)
            .expect("restart request pointer should resolve") = replacement;
        assert!(
            serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(crossed).is_err(),
            "crossed restart field {pointer} must fail closed"
        );
    }
}

#[cfg(unix)]
#[test]
fn workload_restart_phase_wire_rejects_unknown_and_missing_content() {
    let request =
        restart_request_fixture('a', false, MachineApiWorkloadRestartCommandMode::Execute);
    let value = serde_json::to_value(request).expect("restart request should serialize");

    for pointer in ["", "/command", "/command/claim"] {
        let mut unknown = value.clone();
        unknown
            .pointer_mut(pointer)
            .expect("restart object pointer should resolve")
            .as_object_mut()
            .expect("restart pointer should name an object")
            .insert("unexpected".to_owned(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(unknown).is_err(),
            "unknown field at {pointer} must fail closed"
        );
    }

    for field in ["request_digest", "forwarder_authority", "command"] {
        let mut missing = value.clone();
        missing
            .as_object_mut()
            .expect("restart request should be an object")
            .remove(field);
        assert!(
            serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(missing).is_err(),
            "missing restart request field {field} must fail closed"
        );
    }
    for field in [
        "transition_id",
        "source_attempt_id",
        "attempt_id",
        "inspection_version",
        "successor_veto_generation",
        "machine_forwarder_authority",
    ] {
        let mut missing = value.clone();
        missing["command"]
            .as_object_mut()
            .expect("restart command should be an object")
            .remove(field);
        assert!(
            serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(missing).is_err(),
            "missing restart command field {field} must fail closed"
        );
    }
}

#[cfg(unix)]
#[test]
fn workload_restart_phase_response_binds_the_complete_request_and_fences() {
    let request = restart_request_fixture('a', true, MachineApiWorkloadRestartCommandMode::Execute);
    let other = restart_request_fixture('b', false, MachineApiWorkloadRestartCommandMode::Inspect);
    let response = MachineApiWorkloadRestartPhaseResponse::for_request(
        &request,
        MachineApiWorkloadRestartObservation::Succeeded {
            evidence: WorkloadRestartEvidenceDigest::sha256("guest-restart-succeeded"),
        },
    )
    .expect("restart response should correlate");
    let value = serde_json::to_value(&response).expect("restart response should serialize");
    let round_trip =
        serde_json::from_value::<MachineApiWorkloadRestartPhaseResponse>(value.clone())
            .expect("restart response should deserialize");
    round_trip
        .validate_for_request(&request)
        .expect("restart response should retain exact request authority");
    assert_eq!(round_trip, response);
    assert_eq!(response.request_digest(), request.request_digest());
    assert_eq!(
        response.validate_for_request(&other),
        Err(MachineApiWorkloadRestartWireError::ResponseRequestDigestMismatch)
    );

    let other_response = MachineApiWorkloadRestartPhaseResponse::for_request(
        &other,
        MachineApiWorkloadRestartObservation::Ambiguous,
    )
    .expect("other restart response should correlate");
    let other_value =
        serde_json::to_value(other_response).expect("other restart response should serialize");
    for (field, expected) in [
        (
            "request_digest",
            MachineApiWorkloadRestartWireError::ResponseRequestDigestMismatch,
        ),
        (
            "forwarder_authority",
            MachineApiWorkloadRestartWireError::ResponseAuthorityMismatch,
        ),
        (
            "command_id",
            MachineApiWorkloadRestartWireError::ResponseCommandMismatch,
        ),
        (
            "transition_id",
            MachineApiWorkloadRestartWireError::ResponseTransitionMismatch,
        ),
        (
            "request_id",
            MachineApiWorkloadRestartWireError::ResponseRestartRequestMismatch,
        ),
        (
            "source_attempt_id",
            MachineApiWorkloadRestartWireError::ResponseSourceAttemptMismatch,
        ),
        (
            "attempt_id",
            MachineApiWorkloadRestartWireError::ResponseAttemptMismatch,
        ),
        (
            "provider_selection",
            MachineApiWorkloadRestartWireError::ResponseProviderSelectionMismatch,
        ),
    ] {
        let mut crossed = value.clone();
        crossed[field] = other_value[field].clone();
        let decoded = serde_json::from_value::<MachineApiWorkloadRestartPhaseResponse>(crossed)
            .expect("crossed response should remain structurally valid");
        assert_eq!(decoded.validate_for_request(&request), Err(expected));
    }
    for (field, replacement, expected) in [
        (
            "restart_epoch",
            serde_json::json!("2"),
            MachineApiWorkloadRestartWireError::ResponseRestartEpochMismatch,
        ),
        (
            "dispatch_epoch",
            serde_json::json!("1"),
            MachineApiWorkloadRestartWireError::ResponseDispatchEpochMismatch,
        ),
    ] {
        let mut crossed = value.clone();
        crossed[field] = replacement;
        let decoded = serde_json::from_value::<MachineApiWorkloadRestartPhaseResponse>(crossed)
            .expect("crossed response epoch should remain structurally valid");
        assert_eq!(decoded.validate_for_request(&request), Err(expected));
    }

    let mut unknown = value.clone();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<MachineApiWorkloadRestartPhaseResponse>(unknown).is_err());
    let mut unknown_observation = value;
    unknown_observation["observation"]["unexpected"] = serde_json::json!(true);
    assert!(
        serde_json::from_value::<MachineApiWorkloadRestartPhaseResponse>(unknown_observation)
            .is_err()
    );
}

#[cfg(unix)]
fn restart_request_fixture(
    hex: char,
    automatic: bool,
    mode: MachineApiWorkloadRestartCommandMode,
) -> MachineApiWorkloadRestartPhaseRequest {
    let observed = restart_observed_record_fixture(hex);
    let inspection_version =
        automatic.then(|| WorkloadInspectionVersion::from_bytes([hex as u8; 32]));
    let request_id = inspection_version.map_or_else(
        || {
            WorkloadRestartRequestId::for_explicit(
                observed.saga_id(),
                observed.active_intent().source().source_generation(),
                &format!("restart-wire-{hex}"),
            )
            .expect("explicit restart request should validate")
        },
        |version| WorkloadRestartRequestId::for_automatic(observed.saga_id(), version),
    );
    let input = WorkloadRestartAdmissionInput {
        expected_revision: observed.revision(),
        trigger: if automatic {
            WorkloadRestartTrigger::Automatic { exit_code: 17 }
        } else {
            WorkloadRestartTrigger::Explicit
        },
        inspection_version,
        request_id,
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(0),
    };
    let WorkloadRestartAdmissionUpdate::Transition(admitted) = observed
        .admit_restart(input)
        .expect("restart fixture should admit")
    else {
        panic!("new restart fixture should create a transition");
    };
    let request_id = admitted
        .restart_state()
        .active()
        .expect("restart fixture should be active")
        .admission()
        .request_id()
        .clone();
    let quiescence = admitted
        .advance_restart_without_effect(&request_id)
        .expect("withheld restart fixture should enter quiescence");
    let pending = quiescence
        .claim_restart_command(&request_id)
        .expect("restart fixture should claim quiescence");
    let claim = active_restart_claim(&pending);
    let confirmed = match mode {
        MachineApiWorkloadRestartCommandMode::Execute => pending,
        MachineApiWorkloadRestartCommandMode::Inspect => pending
            .restart_dispatch_to_inspection(&claim)
            .expect("restart fixture should retain exact inspection claim"),
    };
    let active = confirmed
        .restart_state()
        .active()
        .expect("confirmed restart fixture should stay active");
    let admission = active.admission();
    let claim = active
        .disposition()
        .claim()
        .expect("confirmed restart fixture should retain a claim")
        .clone();
    let source_execution = confirmed.current_execution_reference();
    let execution = WorkloadExecutionReference::for_restart_epoch(
        confirmed.active_intent(),
        admission.restart_epoch(),
    );
    let machine_generation = NetworkResourceGeneration::new(7);
    let authority = MachineForwarderAuthority::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key(&format!("machine-restart-{hex}")),
            format!("machine-restart-instance-{hex}"),
        )
        .expect("restart forwarder provider should validate"),
        machine_generation,
    );
    let intent = confirmed.active_intent();
    let compiled_network_plan = intent.network().compiled_plan().clone();
    let command = MachineApiWorkloadRestartCommandEnvelope::new(
        claim.command_id().clone(),
        confirmed.key().clone(),
        confirmed.saga_id().clone(),
        confirmed.last_transition().transition_id().clone(),
        admission.generation(),
        admission.desired_digest(),
        admission.source().clone(),
        source_execution.clone(),
        execution.clone(),
        source_execution.attempt_id().clone(),
        execution.attempt_id().clone(),
        admission.restart_epoch(),
        claim.dispatch_epoch(),
        admission.request_id().clone(),
        claim.issuing_revision(),
        confirmed.revision(),
        admission.inspection_version(),
        admission.provider_selection().clone(),
        claim.step(),
        mode,
        None,
        claim,
        intent.executable().clone(),
        compiled_network_plan.plan().digest(),
        compiled_network_plan,
        authority.clone(),
        machine_generation,
    )
    .expect("restart command should validate");
    MachineApiWorkloadRestartPhaseRequest::new(authority, command)
        .expect("restart request should validate")
}

#[cfg(unix)]
fn restart_observed_record_fixture(hex: char) -> WorkloadSagaRecord {
    let intent = restart_intent_fixture(hex);
    let key = WorkloadSagaKey::new(
        intent
            .network()
            .compiled_plan()
            .content()
            .identity()
            .tenant_id()
            .clone(),
        WorkloadId::new(format!("workload-restart-{hex}"))
            .expect("restart workload should validate"),
    );
    let mut record = WorkloadSagaRecord::new(key, intent).expect("restart saga should initialize");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Observed,
    ] {
        record = confirm_restart_provision_edge(&record, phase);
    }
    record
}

#[cfg(unix)]
fn restart_intent_fixture(hex: char) -> WorkloadSagaIntent {
    let tenant_id =
        TenantId::new(format!("tenant-restart-{hex}")).expect("restart tenant should validate");
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"machine-restart-{hex}"}}"#),
    )
    .expect("restart executable should validate");
    let attachment_provider =
        NetworkProviderId::for_registration_key(&format!("restart-attachment-{hex}"));
    let execution_provider =
        WorkloadExecutionProviderId::for_registration_key(&format!("restart-execution-{hex}"));
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            format!("workload-restart-{hex}"),
            format!("profile-restart-{hex}"),
        )
        .expect("restart source identity should validate"),
        WorkloadProvisionSourceGeneration::new(1),
        WorkloadProvisionSourceResourceVersion::new(format!("restart-version-{hex}"))
            .expect("restart source version should validate"),
        executable.content_digest(),
        attachment_provider,
        execution_provider,
    )
    .expect("restart source should validate");
    let generation = WorkloadGeneration::new(1);
    WorkloadSagaIntent::new_with_restart_policy(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        generation,
        executable,
        source,
        WorkloadRestartPolicy::Always { max_restarts: 2 },
        WorkloadNetworkIntent::new(compiled_network_plan_fixture_with_lifecycle(
            &tenant_id,
            hex,
            generation,
            WorkloadActivationIntent::ActivateWhenAttached,
        )),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", hex.to_string().repeat(64))
                .try_into()
                .expect("restart decision should validate"),
            format!("twu_{}", hex.to_string().repeat(64))
                .try_into()
                .expect("restart workload uid should validate"),
            NodeIdentity::new(format!("node-restart-{hex}")).expect("restart node should validate"),
        ),
    )
    .expect("restart intent should validate")
}

#[cfg(unix)]
fn confirm_restart_provision_edge(
    record: &WorkloadSagaRecord,
    target_phase: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    let detail = restart_provision_detail(target_phase, record.active_intent());
    if record.phase() == WorkloadSagaPhase::Ready && target_phase == WorkloadSagaPhase::Observed {
        return record
            .advance(target_phase, detail, None)
            .expect("withheld publication should observe without an effect");
    }
    let intent = record.active_intent();
    let network = WorkloadNetworkReference::for_intent(intent);
    let execution = WorkloadExecutionReference::for_intent(intent);
    if record.phase() == WorkloadSagaPhase::NetworkAttached
        && target_phase == WorkloadSagaPhase::WorkloadActivated
    {
        let inspection = restart_provision_attempt(
            record,
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadProvisionSubjects::Readiness {
                network: network.clone(),
                execution: execution.clone(),
            },
            None,
        );
        let inspection_pending = persist_restart_provision_attempt(record, inspection.clone());
        let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
            inspection.attempt_id().clone(),
            WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
                network,
                execution: execution.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-prerequisites"),
            },
        )
        .expect("restart prerequisite should validate");
        let activation = restart_provision_attempt(
            &inspection_pending,
            WorkloadProvisionStep::ActivateWorkload,
            target_phase,
            WorkloadProvisionSubjects::Execution(execution),
            Some(prerequisite),
        );
        let provider_target = WorkloadProvisionProviderTarget::for_attempt(&activation)
            .expect("activation target should validate")
            .expect("activation should have one execution provider");
        return inspection_pending
            .dispatch_to_activation(activation, provider_target)
            .expect("activation should follow exact prerequisite")
            .dispatch_to_success(target_phase, detail)
            .expect("activation should complete");
    }
    let (step, subjects) = match (record.phase(), target_phase) {
        (WorkloadSagaPhase::IntentCommitted, WorkloadSagaPhase::NetworkReserved) => (
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadProvisionSubjects::Network(network),
        ),
        (WorkloadSagaPhase::NetworkReserved, WorkloadSagaPhase::WorkloadPrepared) => (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(execution),
        ),
        (WorkloadSagaPhase::WorkloadPrepared, WorkloadSagaPhase::NetworkAttached) => (
            WorkloadProvisionStep::AttachNetwork,
            WorkloadProvisionSubjects::Network(network),
        ),
        (WorkloadSagaPhase::WorkloadActivated, WorkloadSagaPhase::Ready) => (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ),
        edge => panic!("unsupported restart provision fixture edge {edge:?}"),
    };
    let attempt = restart_provision_attempt(record, step, target_phase, subjects, None);
    match WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("restart provision target should validate")
    {
        Some(_) => persist_restart_provision_attempt(record, attempt)
            .dispatch_to_success(target_phase, detail)
            .expect("restart provision effect should complete"),
        None => record
            .record_resource_free_network_step(step, target_phase, detail)
            .expect("resource-free restart network step should complete"),
    }
}

#[cfg(unix)]
fn restart_provision_attempt(
    record: &WorkloadSagaRecord,
    step: WorkloadProvisionStep,
    target_phase: WorkloadSagaPhase,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
) -> WorkloadProvisionAttempt {
    let intent = record.active_intent();
    WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        key: record.key().clone(),
        saga_id: record.saga_id().clone(),
        issuing_revision: record.revision(),
        generation: intent.generation(),
        desired_digest: intent.desired_digest(),
        required_node: intent.admission().assigned_node().clone(),
        source_digest: intent.source().source_digest(),
        execution_provider_id: intent.source().execution_provider_id().clone(),
        network_plan_digest: intent.network().digest(),
        selection_evidence: intent
            .network()
            .compiled_plan()
            .content()
            .capability_selection_evidence()
            .cloned(),
        source_phase: record.phase(),
        target_phase,
        step,
        subjects,
        prerequisite,
    })
    .expect("restart provision attempt should validate")
}

#[cfg(unix)]
fn persist_restart_provision_attempt(
    record: &WorkloadSagaRecord,
    attempt: WorkloadProvisionAttempt,
) -> WorkloadSagaRecord {
    let provider_target = WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("restart provision target should validate")
        .expect("effectful restart provision should name a provider");
    record
        .ready_to_initial_dispatch(attempt, provider_target)
        .expect("restart provision attempt should persist")
}

#[cfg(unix)]
fn restart_provision_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
) -> WorkloadPhaseDetail {
    let settled_publication = matches!(
        phase,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Observed
    );
    let publication = settled_publication.then(|| {
        WorkloadPublicationReference::new([], intent)
            .expect("zero-listener restart fixture needs explicit publication authority")
    });
    let references = WorkloadEffectReferences::provision(intent, publication)
        .expect("restart references should validate");
    let network = references
        .network()
        .expect("restart fixture should retain network")
        .clone();
    let execution = references
        .execution()
        .expect("restart fixture should retain execution")
        .clone();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Observed => 5,
        _ => panic!("phase is not restart provision evidence"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadOwnerObservation::NetworkReserved {
            reference: network.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("restart-network-reserved"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("restart-execution-prepared"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("restart-network-attached"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadOwnerObservation::ExecutionActivated {
            reference: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("restart-execution-activated"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadOwnerObservation::Ready {
            network,
            execution,
            evidence: WorkloadOwnerEvidenceDigest::sha256("restart-ready"),
        });
    }
    WorkloadPhaseDetail::provision(phase, intent, references, observations)
        .expect("restart provision detail should validate")
}

#[cfg(unix)]
fn active_restart_claim(record: &WorkloadSagaRecord) -> WorkloadRestartCommandClaim {
    record
        .restart_state()
        .active()
        .expect("restart fixture should be active")
        .disposition()
        .claim()
        .expect("restart fixture should retain a claim")
        .clone()
}

#[cfg(unix)]
fn assert_crossed_restart_request_rejected(
    baseline: &serde_json::Value,
    pointer: &str,
    source: &serde_json::Value,
) {
    let mut crossed = baseline.clone();
    *crossed
        .pointer_mut(pointer)
        .expect("baseline restart pointer should resolve") = source
        .pointer(pointer)
        .expect("source restart pointer should resolve")
        .clone();
    assert!(
        serde_json::from_value::<MachineApiWorkloadRestartPhaseRequest>(crossed).is_err(),
        "crossed restart field {pointer} must fail closed"
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
    let restart_epoch = WorkloadRestartEpoch::new(0);
    let attempt_id = WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch);
    let execution: WorkloadExecutionReference = serde_json::from_value(serde_json::json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "restartEpoch": restart_epoch,
        "attemptId": attempt_id,
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
    compiled_network_plan_fixture_with_lifecycle(
        tenant_id,
        hex,
        generation,
        WorkloadActivationIntent::PrepareOnly,
    )
}

#[cfg(unix)]
fn compiled_network_plan_fixture_with_lifecycle(
    tenant_id: &TenantId,
    hex: char,
    generation: WorkloadGeneration,
    activation: WorkloadActivationIntent,
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
        activation,
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
