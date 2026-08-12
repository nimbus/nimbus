use std::collections::VecDeque;
use std::io::{Read as _, Write as _};
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nimbus::{
    EndpointProtocol, SandboxHandle, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
    SandboxRootSpec, SandboxSpec, SandboxStatus, TenantId,
};
use nimbus_compute::workload_executable::encode_sandbox_spec;
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadProvisionCommand, ConfirmedWorkloadProvisionTransition,
    IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentProvisionCapabilities,
    NetworkReservationCapability, ProposedWorkloadProvisionTransition,
    WorkloadExecutionProvisionCapabilities, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionCommandResult, WorkloadProvisionDecision, WorkloadProvisionDispatcher,
    WorkloadProvisionSourceAuthority, WorkloadProvisionSourceAuthorityError,
    WorkloadProvisionSourceFuture, WorkloadSagaCoordinator, reduce_command_result,
};
use nimbus_core::WorkloadId;
use nimbus_machine::{
    MachineConnectivityCapabilities,
    api::{
        MachineApiServiceSandboxInspectResponse, MachineApiWorkloadProvisionCommandEnvelope,
        MachineApiWorkloadProvisionPhaseRequest, MachineApiWorkloadProvisionPhaseResponse,
    },
};
use nimbus_network::{
    ListenerId, NetworkAttachmentMode, NetworkCapabilityRegistry, NetworkControlPlaneLocality,
    NetworkExposure, NetworkIsolationMode, NetworkManagementMode, NetworkProviderHandle,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyCapabilities, PortBindClaim,
    PortLeaseEffectScope, PortLeaseFence, PortLeaseId, PortLeaseRequest,
};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxInspection, backends::container::OciMachinePortForwarderConfig,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutionReference,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkEndpointSemantics,
    WorkloadNetworkForwardingBehavior, WorkloadNetworkIntent, WorkloadNetworkListenerBlueprint,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode,
    WorkloadOwnerEvidenceDigest, WorkloadProvisionDisposition, WorkloadProvisionEffectResult,
    WorkloadProvisionInspectionResult, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent, WorkloadSagaKey,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};
use tempfile::TempDir;

use super::*;

#[path = "tests/teardown_substitution.rs"]
mod teardown_substitution;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_registry_substitution_publishes_and_observes_exact_forwarded_command() {
    let fixture = Fixture::new([
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"published".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"observed".to_vec(),
        }),
    ]);
    let adapter = Arc::new(fixture.adapter(MachineProvider::Krunkit));
    let source_plan = source_plan(MachineProvider::Krunkit, fixture.authority.clone());
    WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            source_plan.selection().attachment_provider_id().clone(),
            adapter.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            source_plan.execution_provider_id().clone(),
            adapter.clone(),
        )],
        [IngressProvisionCapabilities::new(
            fixture.authority.provider_instance().provider_id().clone(),
            adapter.clone(),
        )],
    )
    .expect("the real forwarded adapter should earn both ingress capabilities");

    let publish = fixture.publish_command().await;
    let published =
        IngressPublicationCapability::execute(adapter.as_ref(), publish.command()).await;
    assert!(matches!(
        published,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let observe = observe_command(&publish).await;
    let observed =
        IngressPublicationInspectionCapability::inspect(adapter.as_ref(), observe.command()).await;
    assert!(
        matches!(
            observed,
            WorkloadProvisionInspectionResult::Succeeded { .. }
        ),
        "exact forwarded publication observation should succeed: {observed:?}"
    );

    let calls = fixture.server.finish();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].command().command_id(),
        &publish.command().command_id()
    );
    assert_eq!(
        calls[1].command().command_id(),
        &observe.command().command_id()
    );
    assert_eq!(
        fixture
            .port_authority
            .list_plan(fixture.compiled_plan.plan().plan_id())
            .expect("parent lease plan should inspect")
            .iter()
            .map(|record| record.phase())
            .collect::<Vec<_>>(),
        [PortLeasePhase::Active]
    );

    let durable = std::fs::read_to_string(
        fixture
            .root
            .path()
            .join("networks/machine-provision-publications/confirmed.json"),
    )
    .expect("confirmed parent journal should exist");
    assert!(durable.contains(&publish.command().command_id().to_string()));
    assert!(durable.contains(publish.command().attempt_id().as_str()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn post_restart_observation_reclaims_live_parent_publication_lifetime() {
    let fixture = Fixture::new([
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"published".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"observed-after-restart".to_vec(),
        }),
    ]);
    let publish = fixture.publish_command().await;
    let first = fixture.adapter(MachineProvider::Krunkit);
    let published = IngressPublicationCapability::execute(&first, publish.command()).await;
    assert!(matches!(
        published,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    drop(first);

    let restarted = fixture.adapter(MachineProvider::Krunkit);
    let observe = observe_command(&publish).await;
    let observed =
        IngressPublicationInspectionCapability::inspect(&restarted, observe.command()).await;
    assert!(matches!(
        observed,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let requests = exact_publication_members(publish.command(), &fixture.authority)
        .into_iter()
        .map(|member| member.request().clone())
        .collect::<Vec<_>>();
    assert!(
        fixture
            .port_authority
            .recover_dead_lifetimes(&requests)
            .is_err(),
        "successful post-restart observation must retain a live process-lifetime owner"
    );
    assert_eq!(fixture.server.finish().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn equal_concurrent_replay_invokes_one_machine_api_effect() {
    let fixture = Fixture::new([ResponseMode::Exact(
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"one-effect".to_vec(),
        },
    )]);
    let adapter = Arc::new(fixture.adapter(MachineProvider::Vfkit));
    let publish = fixture.publish_command().await;
    let command = publish.command();

    let (first, second) = tokio::join!(
        IngressPublicationCapability::execute(adapter.as_ref(), command),
        IngressPublicationCapability::execute(adapter.as_ref(), command)
    );
    assert!(matches!(
        first,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert!(matches!(
        second,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(fixture.server.finish().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parent_port_conflict_fails_before_machine_api_effect() {
    let fixture = Fixture::new([]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let publish = fixture.publish_command().await;
    let member = exact_publication_members(publish.command(), &fixture.authority)
        .into_iter()
        .next()
        .expect("fixture publication should carry one parent listener");
    let existing_request = conflicting_parent_request(member.request());
    let existing_claim = PortBindClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("foreign-live-parent-listener"),
            "foreign-live-parent-listener-incarnation",
        )
        .expect("foreign parent claim should validate"),
    );
    let existing = fixture
        .port_authority
        .reserve_and_claim_bind_with_lifetime(
            existing_request.clone(),
            existing_claim,
            PortLeaseEffectScope::ProviderManaged,
        )
        .expect("foreign parent listener should own the conflicting port");
    let existing_record = existing.record().clone();

    let rejected = IngressPublicationCapability::execute(&adapter, publish.command()).await;

    assert!(matches!(
        rejected,
        WorkloadProvisionInspectionResult::DefiniteFailure { .. }
    ));
    assert_eq!(
        fixture.server.finish().len(),
        0,
        "durable parent conflict must precede every Machine API byte"
    );
    assert_eq!(
        fixture
            .port_authority
            .inspect(existing_request.lease_id())
            .expect("foreign parent owner should inspect"),
        Some(existing_record),
        "the rejected publication must not mutate the live conflicting owner"
    );
    drop(existing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossed_publication_member_fails_before_journal_or_machine_effect() {
    let fixture = Fixture::new([]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let publish = fixture.publish_command().await;
    let validated = adapter
        .validate_publication(
            publish.command(),
            WorkloadProvisionStep::Publish,
            nimbus_workloads::WorkloadProvisionCommandMode::Execute,
        )
        .ok()
        .expect("canonical publication should validate");
    let mut members = validated.members.clone();
    let member = members
        .first_mut()
        .expect("fixture publication should contain one member");
    let binding = member.binding();
    member.replace_binding_for_test(SandboxPortBinding::new(
        "crossed-listener",
        binding.protocol,
        binding.host_port,
        binding.guest_port,
    ));

    let result = adapter.publication_journal.authenticate_or_stage(
        &validated.envelope,
        &validated.authority,
        &members,
    );

    assert!(result.is_err());
    assert_eq!(fixture.server.finish().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_parent_publication_still_stages_exact_nonempty_guest_retirement_authority() {
    let fixture = Fixture::new([]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let prepare = fixture
        .command_at_phase(WorkloadSagaPhase::NetworkReserved)
        .await;
    let command = prepare.command();
    adapter
        .validate_exact_phase(
            command,
            WorkloadProvisionStep::PrepareWorkload,
            nimbus_workloads::WorkloadProvisionCommandMode::Execute,
        )
        .ok()
        .expect("confirmed prepare command should stage retirement authority");
    let sandbox_id = SandboxId::new(command.execution().execution_id().as_str());
    let mut retirement = adapter
        .publication_journal
        .retirement_for(&sandbox_id)
        .expect("retirement authority should inspect")
        .expect("every forwarded workload command must retain retirement authority");

    assert_eq!(retirement.tenant_id(), command.key().tenant_id());
    assert_eq!(retirement.sandbox_id(), &sandbox_id);
    assert_eq!(retirement.forwarder_authority(), &fixture.authority);
    assert!(retirement.members().is_empty());
    let expected_guest_bindings = exact_publication_members(command, &fixture.authority)
        .into_iter()
        .map(|member| member.binding().clone())
        .collect::<Vec<_>>();
    assert!(!expected_guest_bindings.is_empty());
    assert_eq!(
        retirement.expected_guest_bindings(),
        expected_guest_bindings
    );
    assert!(
        fixture
            .port_authority
            .list_plan(fixture.compiled_plan.plan().plan_id())
            .expect("parent lease authority should inspect")
            .is_empty(),
        "the retirement witness must not manufacture parent leases"
    );
    assert!(!retirement.is_retired());

    retirement.replace_expected_guest_bindings_for_test(vec![SandboxPortBinding::tcp(
        "crossed-guest-binding",
        18_181,
        8_181,
    )]);
    assert!(
        adapter
            .publication_journal
            .mark_retired(&retirement)
            .is_err(),
        "crossed guest bindings must fail closed against durable retirement authority"
    );
    assert!(
        !adapter
            .publication_journal
            .retirement_for(&sandbox_id)
            .expect("retirement authority should remain readable")
            .expect("retirement authority should remain staged")
            .is_retired(),
        "a crossed retirement must not mutate durable authority"
    );
    assert_eq!(fixture.server.finish().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn equal_numeric_guest_and_parent_ports_use_distinct_authority_roots() {
    let fixture = Fixture::new([ResponseMode::Exact(
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"parent-published".to_vec(),
        },
    )]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let publish = fixture.publish_command().await;
    let member = exact_publication_members(publish.command(), &fixture.authority)
        .into_iter()
        .next()
        .expect("fixture publication should carry one parent listener");
    let guest_root = fixture.root.path().join("guest-network-authority");
    let guest = LocalPortLeaseAuthority::open(&guest_root)
        .expect("separate guest port authority should open");
    let guest_request = conflicting_parent_request(member.request());
    let guest_record = guest
        .reserve(guest_request.clone())
        .expect("the guest root should admit the same exact numeric port");
    let guest_bytes_before =
        std::fs::read(guest.authority_path()).expect("guest authority snapshot should exist");

    let published = IngressPublicationCapability::execute(&adapter, publish.command()).await;

    assert!(matches!(
        published,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    assert_eq!(fixture.server.finish().len(), 1);
    let parent_records = fixture
        .port_authority
        .list_plan(fixture.compiled_plan.plan().plan_id())
        .expect("parent publication plan should inspect");
    assert_eq!(parent_records.len(), 1);
    assert_eq!(parent_records[0].phase(), PortLeasePhase::Active);
    assert_eq!(
        parent_records[0].reserved_port(),
        guest_record.reserved_port()
    );
    assert_ne!(fixture.port_authority.state_root(), guest.state_root());
    assert_eq!(
        guest
            .inspect(guest_request.lease_id())
            .expect("guest record should inspect"),
        Some(guest_record),
        "parent publication must not mutate equal identity in the guest authority root"
    );
    assert_eq!(
        std::fs::read(guest.authority_path()).expect("guest authority should remain readable"),
        guest_bytes_before,
        "the parent transition must leave the separate guest authority byte-stable"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_only_projection_leaves_parent_authority_bytes_unchanged() {
    let root = TempDir::new().expect("fixture root should exist");
    let parent_root = root.path().join("parent-network-authority");
    let parent =
        LocalPortLeaseAuthority::open(&parent_root).expect("parent port authority should open");
    let authority = forwarder_authority();
    let (intent, _, _) = workload_intent(&authority);
    let execution = WorkloadExecutionReference::for_intent(&intent);
    let sandbox_id = SandboxId::new(execution.execution_id().as_str());
    let inspection = SandboxInspection::provider_reported(SandboxHandle::new(
        tenant(),
        sandbox_id.clone(),
        "machine-api",
        SandboxBackendKind::Container,
        SandboxStatus::Ready,
        Vec::new(),
    ));
    let socket_path = root.path().join("machine-inspect.sock");
    let server = start_inspection_server(
        socket_path.clone(),
        MachineApiServiceSandboxInspectResponse {
            sandbox_id,
            inspection: Some(inspection.clone()),
        },
    );
    let client =
        MachineApiClient::new_for_test(socket_path).with_forwarder_authority(authority.clone());
    let adapter = ForwardedMachineProvisionAdapter::new_for_test(
        client,
        parent.clone(),
        source_plan(MachineProvider::Krunkit, authority),
    )
    .expect("forwarded projection adapter should open");
    let parent_bytes_before = std::fs::read(parent.authority_path()).ok();

    let observed = adapter.observe_execution(
        &workload_key(),
        &execution,
        intent.source(),
        intent.executable(),
    );

    assert_eq!(
        observed,
        WorkloadProviderObservation::Present(inspection),
        "projection must preserve the exact read-only provider observation"
    );
    let request = server.join().expect("inspection server should join");
    assert!(
        request.starts_with("GET /v1/machine-api/service-sandboxes/"),
        "projection must issue only the read endpoint: {request}"
    );
    assert_eq!(
        std::fs::read(parent.authority_path()).ok(),
        parent_bytes_before,
        "read-only projection must not mutate parent lease authority"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn machine_api_and_guest_use_exact_compute_phase_dispatch() {
    let fixture = Fixture::new([
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Ambiguous {
            evidence: b"reserve-reply-lost".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"reserve-inspection-found".to_vec(),
        }),
    ]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let reserve = fixture
        .command_at_phase(WorkloadSagaPhase::IntentCommitted)
        .await;

    let ambiguous = NetworkReservationCapability::execute(&adapter, reserve.command()).await;
    assert!(matches!(
        ambiguous,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));

    let inspection = inspection_command(&reserve).await;
    let recovered = NetworkReservationCapability::inspect(&adapter, inspection.command()).await;
    assert!(
        matches!(
            &recovered,
            WorkloadProvisionInspectionResult::Succeeded { .. }
        ),
        "unexpected recovered reserve result: {recovered:?}"
    );

    let replay = NetworkReservationCapability::execute(&adapter, reserve.command()).await;
    assert!(matches!(
        replay,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));
    let calls = fixture.server.finish();
    assert_eq!(
        calls.len(),
        2,
        "exact replay must not invoke a third effect"
    );
    assert_eq!(
        calls[0].command().mode(),
        WorkloadProvisionCommandMode::Execute
    );
    assert_eq!(
        calls[1].command().mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(
        calls[0].command().attempt_id(),
        calls[1].command().attempt_id()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ambiguous_publish_is_inspected_before_any_retry_effect() {
    let fixture = Fixture::new([
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Ambiguous {
            evidence: b"lost-reply".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"inspection-found-publication".to_vec(),
        }),
    ]);
    let adapter = Arc::new(fixture.adapter(MachineProvider::Krunkit));
    let publish = fixture.publish_command().await;
    let ambiguous =
        IngressPublicationCapability::execute(adapter.as_ref(), publish.command()).await;
    assert!(matches!(
        ambiguous,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));

    let inspection = inspection_command(&publish).await;
    let recovered =
        IngressPublicationCapability::inspect(adapter.as_ref(), inspection.command()).await;
    assert!(
        matches!(
            &recovered,
            WorkloadProvisionInspectionResult::Succeeded { .. }
        ),
        "unexpected recovered publication result: {recovered:?}"
    );

    let calls = fixture.server.finish();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].command().mode(),
        WorkloadProvisionCommandMode::Execute
    );
    assert_eq!(
        calls[1].command().mode(),
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(
        calls[0].command().attempt_id(),
        calls[1].command().attempt_id()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn strict_response_correlation_failure_remains_ambiguous() {
    let fixture = Fixture::new([ResponseMode::CrossedEpoch(
        MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"crossed".to_vec(),
        },
    )]);
    let adapter = fixture.adapter(MachineProvider::Krunkit);
    let publish = fixture.publish_command().await;

    let result = IngressPublicationCapability::execute(&adapter, publish.command()).await;
    assert!(matches!(
        result,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));
    assert_eq!(fixture.server.finish().len(), 1);
    let records = fixture
        .port_authority
        .list_plan(fixture.compiled_plan.plan().plan_id())
        .expect("ambiguous parent lease should inspect");
    assert_eq!(records[0].phase(), PortLeasePhase::Reserved);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossed_forwarder_authority_is_rejected_before_machine_api_effect() {
    let fixture = Fixture::new([ResponseMode::Exact(
        MachineApiWorkloadProvisionObservation::Ambiguous {
            evidence: b"first-authority".to_vec(),
        },
    )]);
    let first = fixture.adapter(MachineProvider::Krunkit);
    let publish = fixture.publish_command().await;
    let first_result = IngressPublicationCapability::execute(&first, publish.command()).await;
    assert!(matches!(
        first_result,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));

    let crossed_authority = MachineForwarderAuthority::new(
        fixture.authority.provider_instance().clone(),
        NetworkResourceGeneration::new(fixture.authority.generation().as_u64() + 1),
    );
    let crossed_client = MachineApiClient::new_for_test(fixture.server.socket_path())
        .with_forwarder_authority(crossed_authority.clone());
    let crossed = ForwardedMachineProvisionAdapter::new_for_test(
        crossed_client,
        fixture.port_authority.clone(),
        source_plan(MachineProvider::Krunkit, crossed_authority),
    )
    .expect("crossed adapter construction itself should be effect-free");
    let rejected = IngressPublicationCapability::execute(&crossed, publish.command()).await;
    assert!(
        matches!(
            &rejected,
            WorkloadProvisionInspectionResult::DefiniteFailure { .. }
        ),
        "unexpected crossed-authority result: {rejected:?}"
    );
    assert_eq!(fixture.server.finish().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_epoch_replay_is_rejected_before_machine_api_effect() {
    let fixture = Fixture::new([
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Ambiguous {
            evidence: b"lost-first-reply".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Absent {
            evidence: b"exactly-absent".to_vec(),
        }),
        ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Succeeded {
            evidence: b"retry-published".to_vec(),
        }),
    ]);
    let adapter = Arc::new(fixture.adapter(MachineProvider::Krunkit));
    let publish = fixture.publish_command().await;
    let ambiguous =
        IngressPublicationCapability::execute(adapter.as_ref(), publish.command()).await;
    assert!(matches!(
        ambiguous,
        WorkloadProvisionInspectionResult::Ambiguous { .. }
    ));

    let inspection = inspection_command(&publish).await;
    let absent =
        IngressPublicationCapability::inspect(adapter.as_ref(), inspection.command()).await;
    assert!(
        matches!(&absent, WorkloadProvisionInspectionResult::Absent { .. }),
        "unexpected publication inspection result: {absent:?}"
    );
    let result = WorkloadProvisionCommandResult::for_command(inspection.command(), absent)
        .expect("exact absence should correlate");
    let WorkloadProvisionDecision::Proposed(retry_proposal) =
        reduce_command_result(inspection.record(), inspection.command(), result)
            .expect("exact absence should authorize one retry")
    else {
        panic!("exact absence should produce a retry proposal");
    };
    let retry = confirm(
        inspection.record(),
        &retry_proposal,
        &inspection.provider_reports,
    )
    .await;
    assert_eq!(retry.command().attempt_id(), publish.command().attempt_id());
    assert_eq!(
        retry.command().dispatch_epoch(),
        publish
            .command()
            .dispatch_epoch()
            .checked_next()
            .expect("the fixture epoch should advance")
    );
    let succeeded = IngressPublicationCapability::execute(adapter.as_ref(), retry.command()).await;
    assert!(matches!(
        succeeded,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));

    let stale = IngressPublicationCapability::inspect(adapter.as_ref(), inspection.command()).await;
    assert!(matches!(
        stale,
        WorkloadProvisionInspectionResult::DefiniteFailure { .. }
    ));
    assert_eq!(fixture.server.finish().len(), 3);
}

#[test]
fn provider_managed_wsl2_fails_before_opening_parent_authority() {
    let root = TempDir::new().expect("fixture root should exist");
    let authority = forwarder_authority();
    let config = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
        authority.provider_instance().expose_to_provider(),
        authority.generation(),
    )
    .expect("fixture forwarder config should validate");
    let error = match ForwardedMachineProvisionSourcePlan::new(
        MachineProvider::Wsl2,
        authority,
        NodeIdentity::new("machine-node").expect("fixture node should validate"),
        source_connectivity(),
        config,
    ) {
        Ok(_) => panic!("provider-managed WSL2 must reject host forwarding"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("host-managed"), "{error}");
    assert!(!root.path().join("networks").exists());
    assert!(!root.path().join(".nimbus-provision-attempts").exists());
}

#[test]
fn forwarded_source_plan_is_deterministic_distinct_and_generation_fenced() {
    let authority = forwarder_authority();
    let first = source_plan(MachineProvider::Krunkit, authority.clone());
    let replay = source_plan(MachineProvider::Krunkit, authority.clone());
    assert_eq!(first, replay);
    assert_eq!(first.digest(), replay.digest());
    assert_eq!(first.bundle().selection(), *first.selection());
    assert_eq!(
        first.selection().attachment_provider_id(),
        &forwarded_machine_attachment_provider_id()
    );
    assert_ne!(
        first.selection().attachment_provider_id(),
        nimbus_sandbox::sandbox_network_plan_requirements(SandboxBackendKind::Container)
            .required_attachment_provider_id(),
        "forwarded attachment authority must not impersonate the local container provider"
    );
    assert_ne!(
        first.execution_provider_id(),
        &nimbus_compute::workload_saga::sandbox_execution_provider_id(
            SandboxBackendKind::Container,
        ),
        "forwarded execution authority must not impersonate the local container provider"
    );
    assert_eq!(first.machine_provider_generation(), authority.generation());
    assert_eq!(first.node_identity().as_str(), "machine-node");
    assert_eq!(
        first.sovereignty().maximum_control_plane_locality(),
        NetworkControlPlaneLocality::LocalOnly
    );

    let next_generation = MachineForwarderAuthority::new(
        authority.provider_instance().clone(),
        NetworkResourceGeneration::new(authority.generation().as_u64() + 1),
    );
    let fenced = source_plan(MachineProvider::Krunkit, next_generation);
    assert_ne!(first.digest(), fenced.digest());
}

#[test]
fn crossed_client_authority_fails_before_opening_adapter_journals() {
    let root = TempDir::new().expect("fixture root should exist");
    let port_authority =
        LocalPortLeaseAuthority::open(root.path()).expect("fixture port authority should open");
    let source_authority = forwarder_authority();
    let crossed_authority = MachineForwarderAuthority::new(
        source_authority.provider_instance().clone(),
        NetworkResourceGeneration::new(source_authority.generation().as_u64() + 1),
    );
    let client = MachineApiClient::new_for_test(root.path().join("never-bound.sock"))
        .with_forwarder_authority(crossed_authority);
    let error = match ForwardedMachineProvisionAdapter::new_for_test(
        client,
        port_authority,
        source_plan(MachineProvider::Krunkit, source_authority),
    ) {
        Ok(_) => panic!("crossed client authority must fail before journal activation"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("generation does not match"),
        "{error}"
    );
    assert!(
        !root
            .path()
            .join("networks/machine-provision-publications")
            .exists()
    );
    assert!(!root.path().join(".nimbus-provision-attempts").exists());
}

struct Fixture {
    root: TempDir,
    server: FakeMachineApi,
    authority: MachineForwarderAuthority,
    port_authority: LocalPortLeaseAuthority,
    intent: WorkloadSagaIntent,
    compiled_plan: CompiledWorkloadNetworkPlan,
    provider_reports: NetworkCapabilityRegistry,
}

impl Fixture {
    fn new(responses: impl IntoIterator<Item = ResponseMode>) -> Self {
        Self::with_listener_count(responses, 1)
    }

    fn with_listener_count(
        responses: impl IntoIterator<Item = ResponseMode>,
        listener_count: usize,
    ) -> Self {
        let root = TempDir::new().expect("fixture root should exist");
        let authority = forwarder_authority();
        let server = FakeMachineApi::start(root.path().join("machine-api.sock"), responses);
        let port_authority =
            LocalPortLeaseAuthority::open(root.path()).expect("fixture port authority should open");
        let (intent, compiled_plan, provider_reports) =
            workload_intent_with_listener_count(&authority, listener_count);
        Self {
            root,
            server,
            authority,
            port_authority,
            intent,
            compiled_plan,
            provider_reports,
        }
    }

    fn adapter(&self, provider: MachineProvider) -> ForwardedMachineProvisionAdapter {
        let client = MachineApiClient::new_for_test(self.server.socket_path())
            .with_forwarder_authority(self.authority.clone());
        ForwardedMachineProvisionAdapter::new_for_test(
            client,
            self.port_authority.clone(),
            source_plan(provider, self.authority.clone()),
        )
        .expect("forwarded adapter should open")
    }

    async fn publish_command(&self) -> ConfirmedFixture {
        self.command_at_phase(WorkloadSagaPhase::Ready).await
    }

    async fn command_at_phase(&self, phase: WorkloadSagaPhase) -> ConfirmedFixture {
        let record = advance_to_phase(
            WorkloadSagaRecord::new(workload_key(), self.intent.clone())
                .expect("fixture saga should validate"),
            phase,
        );
        let proposal = proposal(&record);
        confirm(&record, &proposal, &self.provider_reports).await
    }
}

struct ConfirmedFixture {
    transition: ConfirmedWorkloadProvisionTransition,
    provider_reports: NetworkCapabilityRegistry,
}

impl ConfirmedFixture {
    fn command(&self) -> &ConfirmedWorkloadProvisionCommand {
        self.transition
            .command()
            .expect("confirmed fixture should carry a command")
    }

    fn record(&self) -> &WorkloadSagaRecord {
        self.transition
            .confirmed_record()
            .expect("confirmed fixture should carry durable candidate truth")
    }
}

async fn inspection_command(publish: &ConfirmedFixture) -> ConfirmedFixture {
    let result = WorkloadProvisionCommandResult::for_command(
        publish.command(),
        WorkloadProvisionInspectionResult::Ambiguous {
            attempt_id: publish.command().attempt_id().clone(),
            dispatch_epoch: publish.command().dispatch_epoch(),
            provider_target: publish.command().provider_target().clone(),
        },
    )
    .expect("ambiguous result should correlate");
    let WorkloadProvisionDecision::Proposed(proposal) =
        reduce_command_result(publish.record(), publish.command(), result)
            .expect("ambiguity should propose inspection")
    else {
        panic!("ambiguity should produce an inspection proposal");
    };
    confirm(publish.record(), &proposal, &publish.provider_reports).await
}

async fn observe_command(publish: &ConfirmedFixture) -> ConfirmedFixture {
    let reference = match publish.command().subjects() {
        WorkloadProvisionSubjects::Publication(reference) => reference.clone(),
        _ => panic!("publish command should carry publication subjects"),
    };
    let result = WorkloadProvisionCommandResult::for_command(
        publish.command(),
        WorkloadProvisionInspectionResult::Succeeded {
            attempt_id: publish.command().attempt_id().clone(),
            dispatch_epoch: publish.command().dispatch_epoch(),
            provider_target: publish.command().provider_target().clone(),
            evidence: WorkloadProvisionSuccessEvidence::Published {
                reference,
                evidence: WorkloadOwnerEvidenceDigest::sha256("published"),
            },
        },
    )
    .expect("publish success should correlate");
    let WorkloadProvisionDecision::Proposed(published) =
        reduce_command_result(publish.record(), publish.command(), result)
            .expect("publish success should reduce")
    else {
        panic!("publish success should produce a durable proposal");
    };
    let confirmed_published =
        confirm_transition(publish.record(), &published, &publish.provider_reports).await;
    let published_record = confirmed_published
        .confirmed_record()
        .expect("published transition should retain durable truth");
    let observe = proposal(published_record);
    confirm(published_record, &observe, &publish.provider_reports).await
}

fn proposal(record: &WorkloadSagaRecord) -> ProposedWorkloadProvisionTransition {
    let WorkloadProvisionDecision::Proposed(proposal) =
        WorkloadProvisionDecision::plan(record).expect("fixture phase should be plannable")
    else {
        panic!("fixture phase should produce a proposal");
    };
    proposal
}

async fn confirm(
    record: &WorkloadSagaRecord,
    proposal: &ProposedWorkloadProvisionTransition,
    provider_reports: &NetworkCapabilityRegistry,
) -> ConfirmedFixture {
    ConfirmedFixture {
        transition: confirm_transition(record, proposal, provider_reports).await,
        provider_reports: provider_reports.clone(),
    }
}

async fn confirm_transition(
    record: &WorkloadSagaRecord,
    proposal: &ProposedWorkloadProvisionTransition,
    provider_reports: &NetworkCapabilityRegistry,
) -> ConfirmedWorkloadProvisionTransition {
    let coordinator = WorkloadSagaCoordinator::new(Arc::new(AppliedStore));
    let dispatcher = WorkloadProvisionDispatcher::new(
        Arc::new(StaticSource(record.active_intent().source().clone())),
        provider_reports.clone(),
        Arc::new(
            WorkloadProvisionCapabilityRegistry::new([], [], [])
                .expect("empty fixture capability registry should validate"),
        ),
    );
    dispatcher
        .confirm_transition(&coordinator, record, proposal)
        .await
        .expect("fixture transition should confirm")
}

pub(super) fn advance_to_phase(
    mut record: WorkloadSagaRecord,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    while record.phase() != target {
        let WorkloadProvisionDecision::Proposed(proposal) =
            WorkloadProvisionDecision::plan(&record).expect("fixture phase should plan")
        else {
            panic!("fixture provision phase should propose an attempt");
        };
        let mut candidate = proposal.into_candidate();
        while let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
            candidate.provision_disposition()
        {
            let result = WorkloadProvisionEffectResult::Succeeded {
                attempt_id: claim.attempt().attempt_id().clone(),
                evidence: success_for(claim.attempt().step(), claim.attempt().subjects()),
            };
            let WorkloadProvisionDecision::Proposed(next) =
                WorkloadProvisionDecision::reduce(&candidate, result)
                    .expect("fixture success should reduce")
            else {
                panic!("fixture success should produce a durable candidate");
            };
            candidate = next.into_candidate();
        }
        record = candidate;
    }
    record
}

fn success_for(
    step: WorkloadProvisionStep,
    subjects: &WorkloadProvisionSubjects,
) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("fixture-{step:?}"));
    match (step, subjects) {
        (WorkloadProvisionStep::ReserveNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadProvisionStep::AttachNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkAttached {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference: reference.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::WorkloadReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (WorkloadProvisionStep::Publish, WorkloadProvisionSubjects::Publication(reference)) => {
            WorkloadProvisionSuccessEvidence::Published {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(reference),
        ) => WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference: reference.clone(),
            evidence,
        },
        _ => panic!("unexpected fixture provision step"),
    }
}

pub(super) fn workload_intent(
    authority: &MachineForwarderAuthority,
) -> (
    WorkloadSagaIntent,
    CompiledWorkloadNetworkPlan,
    NetworkCapabilityRegistry,
) {
    workload_intent_with_listener_count(authority, 1)
}

fn workload_intent_with_listener_count(
    authority: &MachineForwarderAuthority,
    listener_count: usize,
) -> (
    WorkloadSagaIntent,
    CompiledWorkloadNetworkPlan,
    NetworkCapabilityRegistry,
) {
    let tenant = tenant();
    let generation = 7;
    let mut spec = SandboxSpec::new(
        tenant.clone(),
        SandboxOwnerSpec::standalone_named("machine-api"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs("/fixture/rootfs"),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    for index in 0..listener_count {
        spec = spec.with_port_binding(
            SandboxPortBinding::new(
                listener_name(index),
                EndpointProtocol::Http,
                38_080 + u16::try_from(index).expect("fixture listener index should fit u16"),
                8_080 + u16::try_from(index).expect("fixture listener index should fit u16"),
            )
            .with_host_address(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        );
    }
    let executable = encode_sandbox_spec(&spec).expect("fixture executable should encode");
    let source_plan = source_plan(MachineProvider::Krunkit, authority.clone());
    let attachment_provider = source_plan.selection().attachment_provider_id().clone();
    let requirements = source_plan.requirements().clone();
    let bundle = source_plan.bundle().clone();
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant.clone(),
        "machine-workload-incarnation",
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture plan identity should validate");
    let attachment = WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
        .expect("fixture attachment should validate");
    let listeners = (0..listener_count)
        .map(|index| {
            let offset = u16::try_from(index).expect("fixture listener index should fit u16");
            WorkloadNetworkListenerBlueprint::new(
                &identity,
                listener_name(index),
                EndpointProtocol::Http,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                WorkloadNetworkPortRequestMode::Exact {
                    port: std::num::NonZeroU16::new(38_080 + offset)
                        .expect("fixture port is non-zero"),
                },
                WorkloadNetworkEndpointSemantics::new(
                    WorkloadNetworkForwardingBehavior::PortForwarded,
                    NetworkTlsBehavior::Disabled,
                ),
                Some(8_080 + offset),
            )
            .expect("fixture listener should validate")
        })
        .collect::<Vec<_>>();
    let selection = bundle.selection();
    let selection_evidence = bundle.selection_evidence();
    let provider_reports =
        NetworkCapabilityRegistry::new([bundle]).expect("fixture provider reports should validate");
    let publication = if listeners.is_empty() {
        WorkloadPublicationIntent::Withheld
    } else {
        WorkloadPublicationIntent::PublishWhenReady
    };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        Some(selection),
        Some(selection_evidence),
        Some(attachment),
        [],
        listeners,
        [],
        WorkloadActivationIntent::ActivateWhenAttached,
        publication,
    )
    .expect("fixture plan content should validate");
    let compiled_plan = CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixture compiled plan should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox("machine-profile", "machine-sandbox")
            .expect("fixture source identity should validate"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new("machine-source-v1")
            .expect("fixture source version should validate"),
        executable.content_digest(),
        attachment_provider,
        source_plan.execution_provider_id().clone(),
    )
    .expect("fixture source evidence should validate");
    let intent = WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        nimbus_workloads::WorkloadGeneration::new(generation),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_plan.clone()),
        WorkloadActivationIntent::ActivateWhenAttached,
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "11".repeat(32))
                .try_into()
                .expect("fixture decision ID should validate"),
            format!("twu_{}", "22".repeat(32))
                .try_into()
                .expect("fixture workload UID should validate"),
            NodeIdentity::new("machine-node").expect("fixture node should validate"),
        ),
    )
    .expect("fixture intent should validate");
    (intent, compiled_plan, provider_reports)
}

fn listener_name(index: usize) -> String {
    if index == 0 {
        "http".to_owned()
    } else {
        format!("http-{index}")
    }
}

fn tenant() -> TenantId {
    TenantId::new("tenant-machine-provision").expect("fixture tenant should validate")
}

pub(super) fn workload_key() -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(),
        WorkloadId::new("machine-workload").expect("fixture workload should validate"),
    )
}

pub(super) fn forwarder_authority() -> MachineForwarderAuthority {
    MachineForwarderAuthority::new(
        OciMachinePortForwarderConfig::gvproxy_provider_handle("gvproxy-test-incarnation")
            .expect("fixture provider handle should validate"),
        NetworkResourceGeneration::new(7),
    )
}

fn source_connectivity() -> MachineConnectivityCapabilities {
    MachineConnectivityCapabilities::new(
        nimbus_network::NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::VirtualMachineGuest],
            [NetworkIsolationMode::WorkloadNamespace],
        ),
        [NetworkExposure::Loopback],
        NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    )
}

pub(super) fn source_plan(
    provider: MachineProvider,
    authority: MachineForwarderAuthority,
) -> ForwardedMachineProvisionSourcePlan {
    let config = OciMachinePortForwarderConfig::gvproxy_for_provider_instance(
        authority.provider_instance().expose_to_provider(),
        authority.generation(),
    )
    .expect("fixture forwarder config should validate");
    ForwardedMachineProvisionSourcePlan::new(
        provider,
        authority,
        NodeIdentity::new("machine-node").expect("fixture node should validate"),
        source_connectivity(),
        config,
    )
    .expect("fixture forwarded source plan should validate")
}

fn exact_publication_members(
    command: &ConfirmedWorkloadProvisionCommand,
    authority: &MachineForwarderAuthority,
) -> Vec<ConfirmedMachinePublicationMember> {
    let envelope = MachineApiWorkloadProvisionCommandEnvelope::new(
        command.command_id(),
        command.attempt_id().clone(),
        command.dispatch_epoch(),
        command.provider_target().clone(),
        command.claim().clone(),
        command.confirmed_revision(),
        command.transition_id().clone(),
        command.generation(),
        command.desired_digest(),
        command.source().clone(),
        command.network_plan_digest(),
        command.execution().clone(),
        command.executable().clone(),
        command.compiled_network_plan().clone(),
        authority.generation(),
        command.mode(),
    )
    .expect("fixture command envelope should validate");
    canonical_machine_publication_members(&envelope, authority)
        .expect("fixture publication should validate")
}

fn conflicting_parent_request(request: &PortLeaseRequest) -> PortLeaseRequest {
    let tenant = request
        .tenant_id()
        .cloned()
        .expect("fixture publication should be tenant-attributed");
    let listener = ListenerId::for_tenant_workload_listener(
        &tenant,
        "foreign-parent-workload",
        "foreign-parent-listener",
    );
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        listener.into(),
        Some(tenant),
        PortLeaseFence::new(request.generation(), request.lease_epoch()),
        request.accounting(),
        request.publication().clone(),
        request.binding().clone(),
    )
}

struct AppliedStore;

struct StaticSource(WorkloadProvisionSourceEvidence);

impl WorkloadProvisionSourceAuthority for StaticSource {
    fn current_source<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
        _identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move { Ok::<_, WorkloadProvisionSourceAuthorityError>(self.0.clone()) })
    }
}

impl WorkloadSagaStore for AppliedStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async { Ok(None) })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async { Ok(WorkloadSagaCommit::Applied) })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

enum ResponseMode {
    Exact(MachineApiWorkloadProvisionObservation),
    CrossedEpoch(MachineApiWorkloadProvisionObservation),
}

fn start_inspection_server(
    path: PathBuf,
    response: MachineApiServiceSandboxInspectResponse,
) -> thread::JoinHandle<String> {
    let listener = UnixListener::bind(path).expect("fake inspection API should bind");
    thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("fake inspection API should accept one request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("fake inspection request timeout should configure");
        let mut request = Vec::new();
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1024];
            let read = stream
                .read(&mut chunk)
                .expect("fake inspection request should read");
            assert!(read > 0, "fake inspection request closed before headers");
            request.extend_from_slice(&chunk[..read]);
        }
        let body = serde_json::to_vec(&response).expect("inspection response should encode");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .expect("inspection response headers should write");
        stream
            .write_all(&body)
            .expect("inspection response body should write");
        String::from_utf8(request).expect("inspection request should be UTF-8")
    })
}

struct FakeMachineApi {
    socket_path: PathBuf,
    calls: Arc<Mutex<Vec<MachineApiWorkloadProvisionPhaseRequest>>>,
    stop: Arc<AtomicBool>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}

impl FakeMachineApi {
    fn start(path: PathBuf, responses: impl IntoIterator<Item = ResponseMode>) -> Self {
        let listener = UnixListener::bind(&path).expect("fake Machine API should bind");
        listener
            .set_nonblocking(true)
            .expect("fake Machine API should become nonblocking");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_calls = calls.clone();
        let worker_stop = stop.clone();
        let mut responses = responses.into_iter().collect::<VecDeque<_>>();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("accepted fake Machine API stream should block");
                        let request = read_request(&mut stream);
                        worker_calls
                            .lock()
                            .expect("fake call log should be healthy")
                            .push(request.clone());
                        let mode = responses.pop_front().unwrap_or_else(|| {
                            ResponseMode::Exact(MachineApiWorkloadProvisionObservation::Ambiguous {
                                evidence: b"unexpected extra call".to_vec(),
                            })
                        });
                        write_response(&mut stream, &request, mode);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("fake Machine API accept failed: {error}"),
                }
            }
        });
        Self {
            socket_path: path,
            calls,
            stop,
            worker: Mutex::new(Some(worker)),
        }
    }

    fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    fn finish(&self) -> Vec<MachineApiWorkloadProvisionPhaseRequest> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self
            .worker
            .lock()
            .expect("fake worker lock should be healthy")
            .take()
        {
            worker.join().expect("fake Machine API should stop cleanly");
        }
        self.calls
            .lock()
            .expect("fake call log should be healthy")
            .clone()
    }
}

impl Drop for FakeMachineApi {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self
            .worker
            .get_mut()
            .expect("fake worker lock should be healthy")
            .take()
        {
            let _ = worker.join();
        }
    }
}

fn read_request(stream: &mut UnixStream) -> MachineApiWorkloadProvisionPhaseRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("fake stream timeout should configure");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 4096];
        let read = stream.read(&mut chunk).expect("fake request should read");
        assert!(read > 0, "fake request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_bytes(&bytes, b"\r\n\r\n") {
            break end + 4;
        }
        assert!(Instant::now() < deadline, "fake request headers timed out");
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).expect("headers should be UTF-8");
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("length should parse"))
            })
        })
        .expect("request should carry content length");
    while bytes.len() < header_end + content_length {
        let mut chunk = [0_u8; 4096];
        let read = stream
            .read(&mut chunk)
            .expect("fake request body should read");
        assert!(read > 0, "fake request closed before its body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length])
        .expect("strict Machine API request should decode")
}

fn write_response(
    stream: &mut UnixStream,
    request: &MachineApiWorkloadProvisionPhaseRequest,
    mode: ResponseMode,
) {
    let (observation, crossed) = match mode {
        ResponseMode::Exact(observation) => (observation, false),
        ResponseMode::CrossedEpoch(observation) => (observation, true),
    };
    let response = MachineApiWorkloadProvisionPhaseResponse::for_request(request, observation)
        .expect("fake response should validate");
    let mut value = serde_json::to_value(response).expect("fake response should encode");
    if crossed {
        let epoch = request.command().dispatch_epoch().as_u64() + 1;
        value["dispatch_epoch"] = serde_json::Value::String(epoch.to_string());
    }
    let body = serde_json::to_vec(&value).expect("fake response JSON should encode");
    write!(
        stream,
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .expect("fake response headers should write");
    stream
        .write_all(&body)
        .expect("fake response body should write");
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
