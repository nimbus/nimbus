use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU16;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use nimbus_compute::workload_saga::{
    WorkloadSagaAction, WorkloadSagaCoordinator, WorkloadSagaDecision,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentMode,
    NetworkBindRealmKind, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkCapabilitySelectionEvidence, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkForwardingFeature,
    NetworkIngressCapabilitySet, NetworkIngressFeature, NetworkIsolationMode,
    NetworkLifecycleCapabilitySet, NetworkLifecycleFeature, NetworkManagementMode,
    NetworkPlanContentDigest, NetworkPortAssignmentMode, NetworkProviderId,
    NetworkResourceGeneration, NetworkSovereigntyRequirements, NetworkTlsBehavior, PortProtocol,
};
use nimbus_testing::{
    ProcessRoleSpec, SubprocessCrashCutHarness, run_crash_cut_child, run_crash_recovery_child,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadGeneration,
    WorkloadNetworkAttachmentBlueprint, WorkloadNetworkDependencyListenerBlueprint,
    WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior, WorkloadNetworkIntent,
    WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadNetworkPortRequestMode, WorkloadNetworkRouteBlueprint, WorkloadProvisionDisposition,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
};
use sha2::{Digest, Sha256};

use super::super::EngineWorkloadSagaStore;
use super::{document_for, provision_source};

const CHILD_TEST: &str =
    "workload_saga_store::tests::compiled_plan_durability::compiled_plan_durability_child";
const MODE_ENV: &str = "NIMBUS_NNC62A_COMPILED_PLAN_MODE";
const WRITE_MODE: &str = "write";
const RECOVER_MODE: &str = "recover";
const BOUNDARY: &str = "workload-saga.compiled-plan-durable";
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_CHILD_DIAGNOSTIC_BYTES: usize = 4 * 1024;
const PID_PREFIX: &str = "NIMBUS_NNC62A_PROCESS_ID";
const FINGERPRINT_PREFIX: &str = "NIMBUS_NNC62A_COMPILED_PLAN_FINGERPRINT";
const EXPECTED_OBSERVATION: &str = "compiled-plan-v1:wire-0b0795fd441c8ac37c8d149ac914eb2caedb44b72c1eba118af1d03ec90d7652:content-4a7e6059e526e63c8356d3ffb67decb69efe8022153e1465e2c42a68176a44e5:plan-ccc17276735408f235495e5ad4a792cf59dc101ef4c7d939583feababa2700e0:a1:r1:l1:d1:q3";
const TENANT: &str = "tenant-nnc62a";
const WORKLOAD: &str = "workload-nnc62a";
const NETWORK_GENERATION: u64 = 7;

#[test]
fn physical_record_retains_one_complete_compiled_network_plan() {
    let (record, compiled) = populated_record();
    let document = document_for(&record);
    let exact_wire = serde_json::to_value(record.active_intent().network())
        .expect("network intent should serialize");

    assert_eq!(
        document.fields.get("compiledNetworkPlan"),
        Some(&exact_wire),
        "physical durable truth must retain the exact complete compiled network plan"
    );
    assert_eq!(
        serde_json::from_value::<WorkloadNetworkIntent>(exact_wire)
            .expect("physical compiled plan should deserialize")
            .compiled_plan(),
        &compiled
    );
    for legacy in ["networkPlanId", "networkGeneration", "networkPlanDigest"] {
        assert!(
            !document.fields.contains_key(legacy),
            "derived tuple field {legacy} must not remain physical desired-state authority"
        );
    }
}

#[test]
fn intent_committed_record_exposes_exact_plan_and_content_bytes() {
    let (record, compiled) = populated_record();
    let network = serde_json::to_value(record.active_intent().network())
        .expect("network intent should serialize");
    let exact_wire = serde_json::to_value(WorkloadNetworkIntent::new(compiled.clone()))
        .expect("fixed network intent should serialize");

    assert_eq!(network, exact_wire);
    assert!(network.pointer("/plan").is_some());
    assert!(network.pointer("/content").is_some());
    let decoded = serde_json::from_value::<WorkloadNetworkIntent>(network)
        .expect("exact network intent bytes should deserialize");
    assert_eq!(decoded.compiled_plan(), &compiled);
    assert_eq!(
        decoded.compiled_plan().content().canonical_bytes(),
        compiled.content().canonical_bytes()
    );
    assert_eq!(record.phase(), WorkloadSagaPhase::IntentCommitted);
    assert!(record.phase_detail().references().is_empty());
}

#[test]
fn fresh_process_recovers_exact_compiled_plan_without_snapshot_handoff() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let result = SubprocessCrashCutHarness::new(TIMEOUT)
        .run(
            root.path(),
            BOUNDARY,
            EXPECTED_OBSERVATION,
            child("compiled-plan-writer", WRITE_MODE),
            child("compiled-plan-recovery", RECOVER_MODE),
        )
        .unwrap_or_else(|error| panic!("compiled network-plan recovery failed: {error}"));

    assert_eq!(result.boundary(), BOUNDARY);
    assert_eq!(result.observation(), EXPECTED_OBSERVATION);
    assert_eq!(
        result.crash_diagnostic().cleanup(),
        "killed-at-boundary-and-reaped"
    );
    assert_eq!(result.crash_diagnostic().successful(), Some(false));
    assert_eq!(result.crash_diagnostic().role(), "compiled-plan-writer");
    assert_eq!(result.recovery_diagnostic().successful(), Some(true));
    assert_eq!(result.recovery_diagnostic().cleanup(), "exited-and-reaped");
    assert_eq!(
        result.recovery_diagnostic().role(),
        "compiled-plan-recovery"
    );

    let writer_pid = process_id(result.crash_diagnostic().stderr(), "writer");
    let recovery_pid = process_id(result.recovery_diagnostic().stderr(), "recovery");
    assert_ne!(
        writer_pid, recovery_pid,
        "recovery must open durable truth in a distinct process"
    );
    assert_eq!(
        fingerprint(result.crash_diagnostic().stderr()),
        EXPECTED_OBSERVATION,
        "the crash-cut writer must report the exact bounded fixture fingerprint"
    );
    assert_eq!(
        result.crash_diagnostic().stderr(),
        format!("{PID_PREFIX} writer {writer_pid}\n{FINGERPRINT_PREFIX} {EXPECTED_OBSERVATION}\n"),
        "the writer diagnostic surface must remain bounded to identity and fingerprint evidence"
    );
    assert_eq!(
        result.recovery_diagnostic().stderr(),
        format!("{PID_PREFIX} recovery {recovery_pid}\n"),
        "the recovery diagnostic surface must retain process identity only"
    );
    for diagnostic in [result.crash_diagnostic(), result.recovery_diagnostic()] {
        assert!(
            diagnostic.stdout().len() <= MAX_CHILD_DIAGNOSTIC_BYTES,
            "{} stdout exceeded the bounded diagnostic contract: {} bytes",
            diagnostic.role(),
            diagnostic.stdout().len()
        );
        assert!(
            diagnostic.stderr().len() <= MAX_CHILD_DIAGNOSTIC_BYTES,
            "{} stderr exceeded the bounded diagnostic contract: {} bytes",
            diagnostic.role(),
            diagnostic.stderr().len()
        );
    }
}

#[test]
#[ignore = "spawned only by the compiled network-plan durability parent"]
fn compiled_plan_durability_child() {
    let mode = std::env::var(MODE_ENV).expect("compiled-plan child mode should be set");
    match mode.as_str() {
        WRITE_MODE => run_crash_cut_child(|context| {
            eprintln!("{PID_PREFIX} writer {}", std::process::id());
            let runtime = runtime()?;
            let engine = Arc::new(
                Engine::new(context.state_root())
                    .map_err(|error| format!("writer Engine open failed: {error}"))?,
            );
            let store = EngineWorkloadSagaStore::new(engine);
            let fingerprint = runtime.block_on(persist_compiled_plan(&store))?;
            eprintln!("{FINGERPRINT_PREFIX} {fingerprint}");
            context.reach_boundary(BOUNDARY)
        })
        .unwrap_or_else(|error| panic!("compiled-plan writer failed: {error}")),
        RECOVER_MODE => run_crash_recovery_child(|context| {
            eprintln!("{PID_PREFIX} recovery {}", std::process::id());
            let runtime = runtime()?;
            runtime.block_on(recover_compiled_plan(context.state_root()))
        })
        .unwrap_or_else(|error| panic!("compiled-plan recovery failed: {error}")),
        unknown => panic!("unknown compiled-plan child mode {unknown:?}"),
    }
}

fn populated_record() -> (WorkloadSagaRecord, CompiledWorkloadNetworkPlan) {
    let compiled = populated_compiled_plan();
    let tenant_id = tenant_id();
    let key = workload_key();
    let executable = nimbus_workloads::WorkloadExecutableIntent::new(
        nimbus_workloads::WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"nnc62a-fixed-desired-workload"}"#,
    )
    .expect("fixed executable should validate");
    let source = provision_source(
        &executable,
        WORKLOAD,
        NETWORK_GENERATION,
        compiled
            .content()
            .capability_selection()
            .expect("populated plan has exact selection")
            .attachment_provider_id()
            .clone(),
    );
    let intent = WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        WorkloadGeneration::new(NETWORK_GENERATION),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled.clone()),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "6".repeat(64))
                .try_into()
                .expect("fixed decision id should validate"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixed workload uid should validate"),
            NodeIdentity::new("node-nnc62a").expect("fixed node id should validate"),
        ),
    )
    .expect("fixed workload intent should validate");
    assert_eq!(intent.network().compiled_plan(), &compiled);
    assert_eq!(intent.network().plan_id(), compiled.plan().plan_id());
    assert_eq!(intent.network().generation(), compiled.plan().generation());
    assert_eq!(intent.network().digest(), compiled.plan().digest());
    assert_eq!(key.tenant_id(), &tenant_id);
    (
        WorkloadSagaRecord::new(key, intent).expect("fixed saga record should validate"),
        compiled,
    )
}

fn populated_compiled_plan() -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id(),
        WORKLOAD,
        NetworkResourceGeneration::new(NETWORK_GENERATION),
    )
    .expect("fixed network identity should validate");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(
            NetworkManagementMode::NimbusHostManaged,
            [NetworkAttachmentMode::IsolatedNamespace],
            [
                NetworkIsolationMode::WorkloadNamespace,
                NetworkIsolationMode::TenantSegment,
            ],
        ),
        NetworkEndpointCapabilitySet::new(
            [NetworkAddressFamily::Ipv4],
            [NetworkBindRealmKind::Host],
            [NetworkExposure::Public],
            [PortProtocol::Tcp],
            [NetworkPortAssignmentMode::Exact],
        ),
        NetworkIngressCapabilitySet::new([
            NetworkIngressFeature::HostRouting,
            NetworkIngressFeature::WebSocket,
            NetworkIngressFeature::Streaming,
        ])
        .with_tls_behaviors([NetworkTlsBehavior::TerminateAtIngress]),
        NetworkForwardingCapabilitySet::new([
            NetworkForwardingFeature::PortForwarding,
            NetworkForwardingFeature::ConnectionDrain,
        ]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
            NetworkLifecycleCapabilitySet::new([
                NetworkLifecycleFeature::DurableInspect,
                NetworkLifecycleFeature::Reconcile,
                NetworkLifecycleFeature::Delete,
            ]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let selection = NetworkCapabilitySelection::new(
        NetworkProviderId::for_registration_key("nnc62a-host-attachment"),
        NetworkProviderId::for_registration_key("nnc62a-host-ingress"),
    );
    let selection_evidence: NetworkCapabilitySelectionEvidence =
        serde_json::from_value(serde_json::json!({
            "selection": selection.clone(),
            "source_digest": "ab".repeat(32),
        }))
        .expect("fixed capability evidence should validate");
    let attachment = WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
        .expect("fixed attachment should validate");
    let route = WorkloadNetworkRouteBlueprint::new(
        &identity,
        "api-service",
        "public-api",
        EndpointProtocol::Https,
        "api.tenant.internal",
        443,
        Some(8443),
    )
    .expect("fixed route should validate");
    let listener = WorkloadNetworkListenerBlueprint::new(
        &identity,
        "public-api",
        EndpointProtocol::Https,
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        WorkloadNetworkPortRequestMode::exact(
            NonZeroU16::new(32_443).expect("fixed listener port is non-zero"),
        ),
        WorkloadNetworkEndpointSemantics::new(
            WorkloadNetworkForwardingBehavior::PortForwarded,
            NetworkTlsBehavior::TerminateAtIngress,
        ),
        Some(8443),
    )
    .expect("fixed listener should validate");
    let dependency = WorkloadNetworkDependencyListenerBlueprint::new(
        &identity,
        "egress-pep",
        NetworkProviderId::for_registration_key("nnc62a-egress-pep"),
    )
    .expect("fixed dependency listener should validate");
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        Some(selection),
        Some(selection_evidence),
        Some(attachment),
        [route],
        [listener],
        [dependency],
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    )
    .expect("fixed complete network-plan content should validate");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixed complete network plan should compile")
}

async fn persist_compiled_plan(store: &EngineWorkloadSagaStore) -> Result<String, String> {
    let (record, compiled) = populated_record();
    if record.phase() != WorkloadSagaPhase::IntentCommitted
        || !record.phase_detail().references().is_empty()
    {
        return Err("writer fixture is not unreserved IntentCommitted truth".to_owned());
    }
    let commit = store
        .compare_and_swap(WorkloadSagaExpected::Missing, record)
        .await
        .map_err(|error| format!("writer compiled-plan persistence failed: {error}"))?;
    if commit != WorkloadSagaCommit::Applied {
        return Err(format!(
            "writer did not newly persist compiled-plan truth: {commit:?}"
        ));
    }
    Ok(observation_for(&compiled))
}

async fn recover_compiled_plan(root: &Path) -> Result<String, String> {
    let engine = Arc::new(
        Engine::new(root).map_err(|error| format!("recovery Engine open failed: {error}"))?,
    );
    let store: Arc<dyn WorkloadSagaStore> = Arc::new(EngineWorkloadSagaStore::new(engine));
    let coordinator = WorkloadSagaCoordinator::new(store);
    let record = coordinator
        .load(&workload_key())
        .await
        .map_err(|error| format!("recovery load failed: {error}"))?
        .ok_or_else(|| "recovery omitted the fixed workload saga".to_owned())?;
    if record.phase() != WorkloadSagaPhase::IntentCommitted
        || !record.phase_detail().references().is_empty()
    {
        return Err("recovered saga is not unreserved IntentCommitted truth".to_owned());
    }

    let (expected_record, expected) = populated_record();
    let recovered_intent = record.active_intent();
    let expected_intent = expected_record.active_intent();
    if recovered_intent != expected_intent
        || recovered_intent.desired_digest() != expected_intent.desired_digest()
        || recovered_intent.source() != expected_intent.source()
        || recovered_intent.source().source_digest() != expected_intent.source().source_digest()
        || recovered_intent.admission().assigned_node()
            != expected_intent.admission().assigned_node()
        || recovered_intent
            .network()
            .compiled_plan()
            .content()
            .capability_selection_evidence()
            != expected_intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
    {
        return Err(
            "fresh Engine changed desired, source, node, selection, or digest evidence".to_owned(),
        );
    }
    let recovered = record.active_intent().network().compiled_plan();
    assert_exact_compiled_plan(recovered, &expected)?;
    let decision = WorkloadSagaDecision::for_record(&record)
        .map_err(|error| format!("recovery decision failed: {error}"))?;
    let WorkloadSagaAction::Provision(
        nimbus_compute::workload_saga::WorkloadProvisionDecision::Proposed(proposed),
    ) = decision.action()
    else {
        return Err(format!(
            "IntentCommitted recovery did not derive ReserveNetwork: {:?}",
            decision.action()
        ));
    };
    let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
        proposed.candidate().provision_disposition()
    else {
        return Err("ReserveNetwork proposal omitted its exact pending attempt".to_owned());
    };
    let attempt = claim.attempt();
    let nimbus_workloads::WorkloadProvisionSubjects::Network(reference) = attempt.subjects() else {
        return Err("ReserveNetwork attempt omitted its network subject".to_owned());
    };
    let nimbus_compute::workload_saga::WorkloadProvisionDecision::Proposed(expected_proposed) =
        nimbus_compute::workload_saga::WorkloadProvisionDecision::plan(&expected_record)
            .map_err(|error| format!("expected attempt derivation failed: {error}"))?
    else {
        return Err("expected record did not derive a ReserveNetwork proposal".to_owned());
    };
    let Some(WorkloadProvisionDisposition::DispatchPending(expected_claim)) =
        expected_proposed.candidate().provision_disposition()
    else {
        return Err("expected proposal omitted its pending attempt".to_owned());
    };
    let expected_attempt = expected_claim.attempt();
    if attempt != expected_attempt
        || attempt.key() != record.key()
        || attempt.saga_id() != record.saga_id()
        || attempt.issuing_revision() != record.revision()
        || attempt.generation() != recovered_intent.generation()
        || attempt.desired_digest() != recovered_intent.desired_digest()
        || attempt.required_node() != recovered_intent.admission().assigned_node()
        || attempt.source_digest() != recovered_intent.source().source_digest()
        || attempt.network_plan_digest() != recovered_intent.network().digest()
        || attempt.selection_evidence()
            != recovered_intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
    {
        return Err(
            "fresh Engine did not reconstruct the byte-exact provision-attempt fences".to_owned(),
        );
    }
    let plan = proposed
        .candidate()
        .active_intent()
        .network()
        .compiled_plan();
    if plan != &expected {
        return Err("ReserveNetwork did not carry exact recovered compiled truth".to_owned());
    }
    if reference.plan_id() != expected.plan().plan_id()
        || reference.generation() != expected.plan().generation()
        || reference.digest() != expected.plan().digest()
    {
        return Err("ReserveNetwork derived tuple does not match compiled truth".to_owned());
    }
    assert_exact_compiled_plan(plan, &expected)?;

    Ok(observation_for(plan))
}

fn assert_exact_compiled_plan(
    candidate: &CompiledWorkloadNetworkPlan,
    expected: &CompiledWorkloadNetworkPlan,
) -> Result<(), String> {
    let candidate_wire = serde_json::to_vec(candidate)
        .map_err(|error| format!("candidate compiled-plan serialization failed: {error}"))?;
    let expected_wire = serde_json::to_vec(expected)
        .map_err(|error| format!("expected compiled-plan serialization failed: {error}"))?;
    let candidate_content = candidate.content().canonical_bytes();
    let expected_content = expected.content().canonical_bytes();

    if candidate != expected
        || candidate_wire != expected_wire
        || candidate_content != expected_content
    {
        return Err("recovered compiled-plan value or exact bytes changed".to_owned());
    }
    if NetworkPlanContentDigest::sha256(&candidate_content) != candidate.plan().content_digest()
        || candidate.plan().content_digest() != expected.plan().content_digest()
        || candidate.plan().digest() != expected.plan().digest()
    {
        return Err("recovered content or complete-plan digest changed".to_owned());
    }
    let content = candidate.content();
    if content.capability_selection() != expected.content().capability_selection()
        || content.capability_requirements() != expected.content().capability_requirements()
        || candidate.plan().readiness_requirements() != expected.plan().readiness_requirements()
        || content.attachment().is_none()
        || content.routes().len() != 1
        || content.listeners().len() != 1
        || content.dependency_listeners().len() != 1
        || content.activation() != WorkloadActivationIntent::ActivateWhenAttached
        || content.publication() != WorkloadPublicationIntent::PublishWhenReady
    {
        return Err(
            "recovered selection, requirements, readiness, resources, or lifecycle intent changed"
                .to_owned(),
        );
    }
    if candidate.plan().readiness_requirements().len() != 3 {
        return Err(format!(
            "recovered readiness cardinality changed: {}",
            candidate.plan().readiness_requirements().len()
        ));
    }
    Ok(())
}

fn observation_for(compiled: &CompiledWorkloadNetworkPlan) -> String {
    let wire_digest = Sha256::digest(
        serde_json::to_vec(compiled).expect("validated compiled network plan always serializes"),
    );
    format!(
        "compiled-plan-v1:wire-{wire_digest:x}:content-{}:plan-{}:a1:r1:l1:d1:q{}",
        compiled.plan().content_digest(),
        compiled.plan().digest(),
        compiled.plan().readiness_requirements().len(),
    )
}

fn tenant_id() -> TenantId {
    TenantId::new(TENANT).expect("fixed tenant id should validate")
}

fn workload_key() -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant_id(),
        WorkloadId::new(WORKLOAD).expect("fixed workload id should validate"),
    )
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| format!("compiled-plan runtime failed: {error}"))
}

fn child(role: &str, mode: &str) -> ProcessRoleSpec {
    ProcessRoleSpec::new(
        role,
        std::env::current_exe().expect("current test executable should resolve"),
    )
    .arg("--exact")
    .arg(CHILD_TEST)
    .arg("--ignored")
    .arg("--nocapture")
    .env(MODE_ENV, mode)
}

fn process_id(stderr: &str, role: &str) -> u32 {
    stderr
        .lines()
        .find_map(|line| {
            line.strip_prefix(&format!("{PID_PREFIX} {role} "))
                .and_then(|value| value.parse().ok())
        })
        .unwrap_or_else(|| panic!("missing {role} child process id in stderr:\n{stderr}"))
}

fn fingerprint(stderr: &str) -> &str {
    stderr
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{FINGERPRINT_PREFIX} ")))
        .unwrap_or_else(|| panic!("missing compiled-plan fingerprint in stderr:\n{stderr}"))
}
