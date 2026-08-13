use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use nimbus_compute::workload_saga::{
    IngressProvisionCapabilities, IngressTeardownCapabilities, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionDecision, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture, WorkloadSagaCoordinator,
    WorkloadTeardownCancellationToken, WorkloadTeardownCapabilityRegistry, WorkloadTeardownRuntime,
};
use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, ListenerId, LocalNetworkAuthority, LocalNetworkManager,
    LocalPortLeaseAuthority, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
    NetworkAttachmentId, NetworkAttachmentProviderRegistration, NetworkCapabilityRegistry,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLeaseEpoch, NetworkLifecycleCapabilitySet,
    NetworkLifecycleRequirements, NetworkManagementMode, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanDigest, NetworkPlanId, NetworkPortAssignmentMode, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, NetworkTlsBehavior,
    PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec, PortExposure,
    PortLeaseAccounting, PortLeaseFence, PortLeaseId, PortLeaseLifetime, PortLeaseRequest,
    PortProtocol, PortPublicationIntent, PortRequestMode, PublishedEndpointId,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadExecutableEncoding,
    WorkloadExecutableIntent, WorkloadExecutionReference, WorkloadNetworkEndpointSemantics,
    WorkloadNetworkForwardingBehavior, WorkloadNetworkIntent, WorkloadNetworkListenerBlueprint,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionDisposition, WorkloadProvisionEffectResult, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadRestartEpoch,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent, WorkloadSagaIntentUpdate,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadTeardownDecision,
};

use crate::EngineWorkloadSagaStore;

use super::*;

static PROCESS_NETWORK_AUTHORITY_TEST_GUARD: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

fn process_network_authority_test_guard() -> tokio::sync::MutexGuard<'static, ()> {
    PROCESS_NETWORK_AUTHORITY_TEST_GUARD.blocking_lock()
}

async fn process_network_authority_test_guard_async() -> tokio::sync::MutexGuard<'static, ()> {
    PROCESS_NETWORK_AUTHORITY_TEST_GUARD.lock().await
}

struct AbsentContainerIngressSource;

impl LocalSandboxIngressTargetSource for AbsentContainerIngressSource {
    fn backend_kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn inspect_targets(
        &self,
        _sandbox_id: &nimbus_sandbox::SandboxId,
        _execution_attempt_id: &nimbus_sandbox::SandboxExecutionAttemptId,
        _network_plan: &SandboxProvisionNetworkPlan,
    ) -> Result<SandboxProvisionIngressTargetObservation, SandboxError> {
        Ok(SandboxProvisionIngressTargetObservation::Absent {
            evidence: b"fixture private attachment absent".to_vec(),
        })
    }
}

fn reservation_claim(label: &str) -> NetworkReservationClaim {
    NetworkReservationClaim::new(
        NetworkProviderHandle::new(
            NetworkProviderId::for_registration_key("nimbus-sandbox.test-attachment"),
            label,
        )
        .expect("fixture provider handle should validate"),
    )
}

fn workload_request(label: &str) -> PortLeaseRequest {
    let listener = ListenerId::for_workload_listener("tenant-a/workload-a", label);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        NetworkResourceId::from(listener),
        Some(nimbus_core::TenantId::new("tenant-a").expect("fixture tenant should parse")),
        PortLeaseFence::new(NetworkResourceGeneration::new(7), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::ProviderAssigned,
        ),
    )
}

#[test]
fn real_server_ingress_adapter_substitutes_for_publication_inspection_and_observation() {
    let _process_authority_guard = process_network_authority_test_guard();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("fixture network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(
        NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"),
    ));
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            manager.authority(),
        )
        .expect("server ingress journal should open"),
    );

    WorkloadProvisionCapabilityRegistry::new(
        [],
        [],
        [IngressProvisionCapabilities::new(
            nimbus_owned_local_ingress_provider_id(),
            adapter,
        )],
    )
    .expect("the real server adapter should earn all three narrow ingress capabilities");
}

#[test]
fn real_server_ingress_adapter_substitutes_for_final_withdrawal_capability() {
    let _process_authority_guard = process_network_authority_test_guard();
    let root = tempfile::tempdir().expect("fixture root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(root.path())
        .expect("fixture network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(
        NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"),
    ));
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            manager.authority(),
        )
        .expect("server ingress journal should open"),
    );

    WorkloadTeardownCapabilityRegistry::new(
        [],
        [],
        [IngressTeardownCapabilities::new(
            nimbus_owned_local_ingress_provider_id(),
            adapter,
        )],
    )
    .expect("the real server adapter should earn final ingress withdrawal authority");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_teardown_runtime_advances_exact_final_ingress_withdrawal() {
    let _process_authority_guard = process_network_authority_test_guard_async().await;
    let root = tempfile::tempdir().expect("runtime fixture root should exist");
    let bundle = runtime_network_bundle();
    let reports = NetworkCapabilityRegistry::new([bundle.clone()])
        .expect("runtime provider reports should validate");
    let bootstrap = LocalNetworkManager::bootstrap(root.path().join("network"))
        .expect("runtime network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(reports.clone()));
    let network_authority = manager.authority();
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("runtime server ingress journal should open"),
    );

    let history = runtime_withdrawal_history(&bundle);
    let observed = history
        .iter()
        .find(|record| record.phase() == WorkloadSagaPhase::Observed)
        .expect("runtime history should retain observed publication");
    install_runtime_live_batch(&adapter, &network_authority, observed);
    let addresses = adapter
        .running
        .lock()
        .expect("runtime live registry should remain healthy")
        .values()
        .flat_map(|batch| batch.routes.iter().map(|route| route.bound_addr))
        .collect::<Vec<_>>();

    let engine = Arc::new(
        nimbus_engine::Engine::new(root.path().join("engine"))
            .expect("runtime fixture Engine should open"),
    );
    let store = Arc::new(EngineWorkloadSagaStore::new(engine));
    persist_runtime_history(store.as_ref(), &history).await;
    let current = history
        .last()
        .expect("runtime history should end at withdrawal intent");
    let source = Arc::new(RuntimeSourceAuthority {
        key: current.key().clone(),
        identity: current.active_intent().source().source_identity().clone(),
        evidence: current.active_intent().source().clone(),
    });
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let capabilities = Arc::new(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [],
            [IngressTeardownCapabilities::new(
                nimbus_owned_local_ingress_provider_id(),
                adapter.clone(),
            )],
        )
        .expect("runtime should register the real final ingress capability"),
    );
    let runtime = WorkloadTeardownRuntime::new(coordinator, source, reports, capabilities);
    runtime
        .submit(
            current.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
        .expect_err("the focused registry intentionally omits the later drain capability");

    let advanced = store
        .load(current.key())
        .await
        .expect("advanced runtime record should load")
        .expect("advanced runtime record should remain durable");
    assert_eq!(advanced.phase(), WorkloadSagaPhase::Withdrawn);
    assert!(
        adapter
            .running
            .lock()
            .expect("runtime live registry should remain healthy")
            .values()
            .all(|batch| batch.final_phase == FinalIngressPhase::Released)
    );
    for address in addresses {
        drop(
            TcpListener::bind(address)
                .expect("runtime success must close and release every exact listener"),
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum CrossedRuntimeIngressAuthority {
    GenerationAndPlan,
    ExecutionAttempt,
    EndpointSet,
    ProviderEvidence,
    WorkloadSource,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossed_runtime_ingress_fences_select_no_route_and_make_zero_effects() {
    let _process_authority_guard = process_network_authority_test_guard_async().await;
    let root = tempfile::tempdir().expect("crossed runtime fixture root should exist");
    for case in [
        CrossedRuntimeIngressAuthority::GenerationAndPlan,
        CrossedRuntimeIngressAuthority::ExecutionAttempt,
        CrossedRuntimeIngressAuthority::EndpointSet,
        CrossedRuntimeIngressAuthority::ProviderEvidence,
        CrossedRuntimeIngressAuthority::WorkloadSource,
    ] {
        assert_crossed_runtime_ingress_is_effect_free(root.path(), case).await;
    }
}

async fn assert_crossed_runtime_ingress_is_effect_free(
    root: &Path,
    case: CrossedRuntimeIngressAuthority,
) {
    let case_root = root.join(format!("{case:?}"));
    let network_root = case_root.join("network");
    let bundle = runtime_network_bundle();
    let reports = NetworkCapabilityRegistry::new([bundle.clone()])
        .expect("crossed runtime provider reports should validate");
    let bootstrap = LocalNetworkManager::bootstrap(&network_root)
        .expect("crossed runtime network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(reports.clone()));
    let network_authority = manager.authority();
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("crossed runtime server ingress journal should open"),
    );
    let history = runtime_withdrawal_history(&bundle);
    let observed = history
        .iter()
        .find(|record| record.phase() == WorkloadSagaPhase::Observed)
        .expect("crossed runtime history should retain observed publication");
    install_runtime_live_batch(&adapter, &network_authority, observed);

    let exact_reference = observed
        .phase_detail()
        .references()
        .publication()
        .expect("crossed runtime record should retain publication")
        .clone();
    let exact_provider_source = observed
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .capability_selection_evidence()
        .expect("crossed runtime plan should retain provider evidence")
        .source_digest();
    let exact_workload_source = observed.active_intent().source().source_digest();
    let crossed_authority = match case {
        CrossedRuntimeIngressAuthority::GenerationAndPlan => {
            let crossed_intent = runtime_intent(
                observed.key(),
                2,
                DesiredWorkloadState::Running,
                WorkloadPublicationIntent::PublishWhenReady,
                &bundle,
            );
            PublishedIngressAuthority::new(
                publication_reference_for_intent(&crossed_intent),
                exact_provider_source,
                exact_workload_source,
            )
        }
        CrossedRuntimeIngressAuthority::ExecutionAttempt => {
            let execution = WorkloadExecutionReference::for_restart_epoch(
                observed.active_intent(),
                WorkloadRestartEpoch::new(1),
            );
            PublishedIngressAuthority::new(
                nimbus_workloads::WorkloadPublicationReference::for_execution(
                    exact_reference.endpoints().iter().cloned(),
                    observed.active_intent(),
                    execution,
                )
                .expect("crossed runtime attempt should form a valid publication"),
                exact_provider_source,
                exact_workload_source,
            )
        }
        CrossedRuntimeIngressAuthority::EndpointSet => PublishedIngressAuthority::new(
            nimbus_workloads::WorkloadPublicationReference::for_execution(
                [PublishedEndpointId::for_workload_endpoint(
                    "tenant-runtime-ingress/runtime-ingress",
                    "crossed-endpoint",
                )],
                observed.active_intent(),
                exact_reference.execution().clone(),
            )
            .expect("crossed runtime endpoint set should form a valid publication"),
            exact_provider_source,
            exact_workload_source,
        ),
        CrossedRuntimeIngressAuthority::ProviderEvidence => PublishedIngressAuthority::new(
            exact_reference.clone(),
            nimbus_network::NetworkCapabilitySourceDigest::from_bytes([0x5a; 32]),
            exact_workload_source,
        ),
        CrossedRuntimeIngressAuthority::WorkloadSource => PublishedIngressAuthority::new(
            exact_reference,
            exact_provider_source,
            nimbus_workloads::WorkloadProvisionSourceDigest::sha256("crossed-runtime-source"),
        ),
    };
    let (addresses, before_routes) = {
        let mut running = adapter
            .running
            .lock()
            .expect("crossed runtime live registry should remain healthy");
        let batch = running
            .values_mut()
            .next()
            .expect("crossed runtime should retain one exact live batch");
        batch.publication = crossed_authority;
        (
            batch
                .routes
                .iter()
                .map(|route| route.bound_addr)
                .collect::<Vec<_>>(),
            batch.routes.len(),
        )
    };

    let engine = Arc::new(
        nimbus_engine::Engine::new(case_root.join("engine"))
            .expect("crossed runtime fixture Engine should open"),
    );
    let store = Arc::new(EngineWorkloadSagaStore::new(engine));
    persist_runtime_history(store.as_ref(), &history).await;
    let current = history
        .last()
        .expect("crossed runtime history should end at withdrawal intent");
    let source = Arc::new(RuntimeSourceAuthority {
        key: current.key().clone(),
        identity: current.active_intent().source().source_identity().clone(),
        evidence: current.active_intent().source().clone(),
    });
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let capabilities = Arc::new(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [],
            [IngressTeardownCapabilities::new(
                nimbus_owned_local_ingress_provider_id(),
                adapter.clone(),
            )],
        )
        .expect("crossed runtime should register the real final ingress capability"),
    );
    let runtime = WorkloadTeardownRuntime::new(coordinator, source, reports, capabilities);
    let leases_before = network_authority
        .port_leases()
        .list()
        .expect("crossed runtime leases should remain readable");
    let files_before = snapshot_regular_files(&network_root);

    let _ = runtime
        .submit(
            current.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await;

    let after = store
        .load(current.key())
        .await
        .expect("crossed runtime record should load")
        .expect("crossed runtime record should remain durable");
    assert_ne!(after.phase(), WorkloadSagaPhase::Withdrawn, "case {case:?}");
    let running = adapter
        .running
        .lock()
        .expect("crossed runtime live registry should remain healthy");
    let batch = running
        .values()
        .next()
        .expect("crossed runtime must retain the live batch");
    assert_eq!(
        batch.final_phase,
        FinalIngressPhase::Published,
        "case {case:?}"
    );
    assert_eq!(batch.routes.len(), before_routes, "case {case:?}");
    assert!(
        batch.routes.iter().all(RunningIngressRoute::is_healthy),
        "case {case:?}"
    );
    drop(running);
    assert_eq!(
        network_authority
            .port_leases()
            .list()
            .expect("crossed runtime leases should remain readable"),
        leases_before,
        "crossed case {case:?} must not change durable lease state"
    );
    assert_eq!(
        snapshot_regular_files(&network_root),
        files_before,
        "crossed case {case:?} must not change network authority bytes"
    );
    for address in addresses {
        assert!(TcpStream::connect(address).is_ok(), "case {case:?}");
        assert_eq!(
            TcpListener::bind(address)
                .expect_err("crossed final withdrawal must retain every bound listener")
                .kind(),
            std::io::ErrorKind::AddrInUse,
            "case {case:?}"
        );
    }
}

fn publication_reference_for_intent(
    intent: &WorkloadSagaIntent,
) -> nimbus_workloads::WorkloadPublicationReference {
    nimbus_workloads::WorkloadPublicationReference::new(
        intent
            .network()
            .compiled_plan()
            .content()
            .listeners()
            .iter()
            .map(|listener| listener.endpoint_id().clone()),
        intent,
    )
    .expect("runtime publication reference should validate")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inspection_reconciles_dead_listener_owners_but_not_live_owners() {
    let _process_authority_guard = process_network_authority_test_guard_async().await;
    let root = tempfile::tempdir().expect("inspection fixture root should exist");
    for case in [
        RuntimeInspectionOwnerCase::RetainedDeadBatch,
        RuntimeInspectionOwnerCase::FreshDeadOwner,
        RuntimeInspectionOwnerCase::LiveOtherOwner,
    ] {
        assert_runtime_inspection_owner_case(root.path(), case).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_process_same_cardinality_crossed_endpoint_reference_preserves_exact_listener_leases()
{
    let _process_authority_guard = process_network_authority_test_guard_async().await;
    let root = tempfile::tempdir().expect("crossed membership fixture root should exist");
    let network_root = root.path().join("network");
    let bundle = runtime_network_bundle();
    let reports = NetworkCapabilityRegistry::new([bundle.clone()])
        .expect("crossed membership provider reports should validate");
    let bootstrap = LocalNetworkManager::bootstrap(&network_root)
        .expect("crossed membership network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(reports.clone()));
    let network_authority = manager.authority();
    let owner_adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("crossed membership owner adapter should open"),
    );
    let owner_history = runtime_withdrawal_history_for_listeners(&bundle, &["http", "admin"]);
    let owner_observed = owner_history
        .iter()
        .find(|record| record.phase() == WorkloadSagaPhase::Observed)
        .expect("owner history should retain observed publication");
    install_runtime_live_batch(&owner_adapter, &network_authority, owner_observed);
    let owner_reference = owner_observed
        .phase_detail()
        .references()
        .publication()
        .expect("owner history should retain exact endpoints")
        .clone();
    let owner_requests = {
        let mut running = owner_adapter
            .running
            .lock()
            .expect("owner live registry should remain healthy");
        let (_, mut batch) = running
            .pop_first()
            .expect("owner adapter should retain one live batch");
        batch.routes[0].inject_final_join_failure_for_test();
        batch
            .stop_and_release_for_final_withdrawal()
            .expect_err("post-join ambiguity should retain every owner lease fence");
        let requests = batch
            .routes
            .iter()
            .map(|route| route.expected.request.clone())
            .collect::<Vec<_>>();
        drop(batch);
        requests
    };

    let crossed_history =
        runtime_withdrawal_history_for_listeners(&bundle, &["crossed-http", "crossed-admin"]);
    let crossed_observed = crossed_history
        .iter()
        .find(|record| record.phase() == WorkloadSagaPhase::Observed)
        .expect("crossed history should retain observed publication");
    let crossed_reference = crossed_observed
        .phase_detail()
        .references()
        .publication()
        .expect("crossed history should retain foreign endpoints")
        .clone();
    assert_eq!(
        owner_reference.network().plan_id(),
        crossed_reference.network().plan_id(),
        "the fail-before requires the same stable plan identity"
    );
    assert_ne!(
        owner_reference.network().digest(),
        crossed_reference.network().digest(),
        "the foreign compiled content must retain a distinct digest"
    );
    assert_eq!(
        owner_reference.endpoints().len(),
        crossed_reference.endpoints().len(),
        "the crossed endpoint sets must have equal cardinality"
    );
    assert!(
        owner_reference
            .endpoints()
            .iter()
            .all(|endpoint| !crossed_reference.endpoints().contains(endpoint)),
        "the crossed endpoint set must not alias a real owner endpoint"
    );

    let inspecting_adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("fresh crossed membership adapter should open"),
    );
    let engine = Arc::new(
        nimbus_engine::Engine::new(root.path().join("engine"))
            .expect("crossed membership Engine should open"),
    );
    let store = Arc::new(EngineWorkloadSagaStore::new(engine));
    persist_runtime_history(store.as_ref(), &crossed_history).await;
    let current = crossed_history
        .last()
        .expect("crossed membership history should end at withdrawal intent");
    let source = Arc::new(RuntimeSourceAuthority {
        key: current.key().clone(),
        identity: current.active_intent().source().source_identity().clone(),
        evidence: current.active_intent().source().clone(),
    });
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let capabilities = Arc::new(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [],
            [IngressTeardownCapabilities::new(
                nimbus_owned_local_ingress_provider_id(),
                inspecting_adapter.clone(),
            )],
        )
        .expect("crossed membership runtime should register real ingress"),
    );
    let runtime = WorkloadTeardownRuntime::new(coordinator, source, reports, capabilities);
    let leases_before = network_authority
        .port_leases()
        .list()
        .expect("crossed membership leases should remain readable");
    let files_before = snapshot_regular_files(&network_root);

    let _ = runtime
        .submit(
            current.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await;

    let advanced = store
        .load(current.key())
        .await
        .expect("crossed membership record should load")
        .expect("crossed membership record should remain durable");
    assert_ne!(
        advanced.phase(),
        WorkloadSagaPhase::Withdrawn,
        "foreign endpoint membership must not withdraw real listener authority"
    );
    assert!(
        matches!(
            advanced.teardown_disposition(),
            Some(nimbus_workloads::WorkloadTeardownDisposition::DefiniteFailure { .. })
        ),
        "crossed durable listener membership must produce a definite rejection"
    );
    assert_eq!(
        network_authority
            .port_leases()
            .list()
            .expect("crossed membership leases should remain readable"),
        leases_before,
        "foreign endpoint membership must preserve every listener lease"
    );
    assert_eq!(
        snapshot_regular_files(&network_root),
        files_before,
        "foreign endpoint membership must preserve durable network bytes"
    );
    for request in owner_requests {
        assert_eq!(
            network_authority
                .port_leases()
                .inspect(request.lease_id())
                .expect("owner listener should inspect")
                .expect("owner listener history should remain durable")
                .phase(),
            PortLeasePhase::Withdrawing
        );
    }
    assert!(
        inspecting_adapter
            .running
            .lock()
            .expect("fresh adapter registry should remain healthy")
            .is_empty(),
        "fresh reconciliation must not start a route or worker"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuntimeInspectionOwnerCase {
    RetainedDeadBatch,
    FreshDeadOwner,
    LiveOtherOwner,
}

async fn assert_runtime_inspection_owner_case(root: &Path, case: RuntimeInspectionOwnerCase) {
    let case_root = root.join(format!("{case:?}"));
    let network_root = case_root.join("network");
    let bundle = runtime_network_bundle();
    let reports = NetworkCapabilityRegistry::new([bundle.clone()])
        .expect("inspection provider reports should validate");
    let bootstrap = LocalNetworkManager::bootstrap(&network_root)
        .expect("inspection network authority should bootstrap");
    let manager = Arc::new(bootstrap.freeze(reports.clone()));
    let network_authority = manager.authority();
    let owner_adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("owner ingress adapter should open"),
    );
    let history = runtime_inspection_history(&bundle);
    let observed = history
        .iter()
        .find(|record| record.phase() == WorkloadSagaPhase::Observed)
        .expect("inspection history should retain observed publication");
    install_runtime_live_batch(&owner_adapter, &network_authority, observed);
    let (addresses, requests) = {
        let mut running = owner_adapter
            .running
            .lock()
            .expect("owner live registry should remain healthy");
        let batch = running
            .values_mut()
            .next()
            .expect("owner adapter should retain one live batch");
        let leases = batch
            .routes
            .iter()
            .map(|route| {
                route
                    .lease
                    .as_ref()
                    .expect("owner route should retain listener authority")
            })
            .collect::<Vec<_>>();
        withdraw_server_listeners_for_final_withdrawal(&batch.plan_members, &leases)
            .expect("inspection fixture should persist exact listener withdrawal");
        (
            batch
                .routes
                .iter()
                .map(|route| route.bound_addr)
                .collect::<Vec<_>>(),
            batch
                .routes
                .iter()
                .map(|route| route.expected.request.clone())
                .collect::<Vec<_>>(),
        )
    };
    match case {
        RuntimeInspectionOwnerCase::LiveOtherOwner => {
            owner_adapter
                .running
                .lock()
                .expect("owner live registry should remain healthy")
                .values_mut()
                .next()
                .expect("owner adapter should retain one live batch")
                .final_phase = FinalIngressPhase::Withdrawing;
        }
        RuntimeInspectionOwnerCase::RetainedDeadBatch => {
            let mut running = owner_adapter
                .running
                .lock()
                .expect("owner live registry should remain healthy");
            let batch = running
                .values_mut()
                .next()
                .expect("owner adapter should retain one live batch");
            batch.routes[0].inject_final_join_failure_for_test();
            batch
                .stop_and_release_for_final_withdrawal()
                .expect_err("injected post-join ambiguity should retain withdrawal fences");
        }
        RuntimeInspectionOwnerCase::FreshDeadOwner => {
            let (_, mut batch) = owner_adapter
                .running
                .lock()
                .expect("owner live registry should remain healthy")
                .pop_first()
                .expect("owner adapter should retain one live batch");
            batch.routes[0].inject_final_join_failure_for_test();
            batch
                .stop_and_release_for_final_withdrawal()
                .expect_err("injected post-join ambiguity should retain withdrawal fences");
            drop(batch);
        }
    }

    let inspecting_adapter = if case == RuntimeInspectionOwnerCase::RetainedDeadBatch {
        owner_adapter.clone()
    } else {
        Arc::new(
            ServerIngressPublicationAdapter::new(
                Arc::new(AbsentContainerIngressSource),
                network_authority.clone(),
            )
            .expect("fresh inspecting ingress adapter should open"),
        )
    };
    let engine = Arc::new(
        nimbus_engine::Engine::new(case_root.join("engine"))
            .expect("inspection fixture Engine should open"),
    );
    let store = Arc::new(EngineWorkloadSagaStore::new(engine));
    persist_runtime_history(store.as_ref(), &history).await;
    let current = history
        .last()
        .expect("inspection history should end in inspection-required state");
    let source = Arc::new(RuntimeSourceAuthority {
        key: current.key().clone(),
        identity: current.active_intent().source().source_identity().clone(),
        evidence: current.active_intent().source().clone(),
    });
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store.clone()));
    let capabilities = Arc::new(
        WorkloadTeardownCapabilityRegistry::new(
            [],
            [],
            [IngressTeardownCapabilities::new(
                nimbus_owned_local_ingress_provider_id(),
                inspecting_adapter,
            )],
        )
        .expect("inspection runtime should register the real ingress capability"),
    );
    let runtime = WorkloadTeardownRuntime::new(coordinator, source, reports, capabilities);
    let leases_before = network_authority
        .port_leases()
        .list()
        .expect("inspection leases should remain readable");
    let files_before = snapshot_regular_files(&network_root);

    let _ = runtime
        .submit(
            current.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await;

    let advanced = store
        .load(current.key())
        .await
        .expect("inspection runtime record should load")
        .expect("inspection runtime record should remain durable");
    if case == RuntimeInspectionOwnerCase::LiveOtherOwner {
        assert_eq!(advanced.phase(), current.phase());
        assert_eq!(
            network_authority
                .port_leases()
                .list()
                .expect("live-owner leases should remain readable"),
            leases_before,
            "inspection must not settle another live owner's listeners"
        );
        assert_eq!(snapshot_regular_files(&network_root), files_before);
        let running = owner_adapter
            .running
            .lock()
            .expect("live owner registry should remain healthy");
        assert!(
            running
                .values()
                .flat_map(|batch| &batch.routes)
                .all(RunningIngressRoute::is_healthy)
        );
        drop(running);
        for address in addresses {
            assert!(TcpStream::connect(address).is_ok());
            assert_eq!(
                TcpListener::bind(address)
                    .expect_err("live-owner inspection must retain listener ownership")
                    .kind(),
                std::io::ErrorKind::AddrInUse
            );
        }
    } else {
        assert_eq!(advanced.phase(), WorkloadSagaPhase::Withdrawn);
        for (request, address) in requests.iter().zip(addresses) {
            let record = network_authority
                .port_leases()
                .inspect(request.lease_id())
                .expect("reconciled listener should inspect")
                .expect("reconciled listener history should remain durable");
            assert_eq!(record.phase(), PortLeasePhase::Released);
            drop(
                TcpListener::bind(address)
                    .expect("dead-owner inspection should release without binding a listener"),
            );
        }
    }
}

fn runtime_inspection_history(
    bundle: &nimbus_network::NetworkCapabilityBundle,
) -> Vec<WorkloadSagaRecord> {
    let mut history = runtime_withdrawal_history(bundle);
    let current = history
        .last()
        .expect("inspection history should retain withdrawal intent");
    let WorkloadTeardownDecision::PersistCandidate(
        nimbus_workloads::ProposedWorkloadTeardownTransition::Claim {
            attempt,
            provider_target,
        },
    ) = current
        .decide_teardown()
        .expect("withdrawal intent should reduce")
    else {
        panic!("withdrawal intent should propose the final ingress claim");
    };
    let pending = current
        .claim_teardown(*attempt, provider_target)
        .expect("final ingress claim should validate");
    let claim = pending
        .teardown_disposition()
        .and_then(nimbus_workloads::WorkloadTeardownDisposition::claim)
        .expect("pending final ingress should retain its exact claim")
        .clone();
    history.push(pending.clone());
    history.push(
        pending
            .teardown_dispatch_to_inspection(&claim)
            .expect("pending final ingress should enter inspection"),
    );
    history
}

struct RuntimeSourceAuthority {
    key: nimbus_workloads::WorkloadSagaKey,
    identity: nimbus_workloads::WorkloadProvisionSourceIdentity,
    evidence: WorkloadProvisionSourceEvidence,
}

impl WorkloadProvisionSourceAuthority for RuntimeSourceAuthority {
    fn current_source<'a>(
        &'a self,
        key: &'a nimbus_workloads::WorkloadSagaKey,
        identity: &'a nimbus_workloads::WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            if key != &self.key || identity != &self.identity {
                return Err(WorkloadProvisionSourceAuthorityError::Corrupt);
            }
            Ok(self.evidence.clone())
        })
    }
}

async fn persist_runtime_history(store: &EngineWorkloadSagaStore, history: &[WorkloadSagaRecord]) {
    for (index, record) in history.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |prior| {
                WorkloadSagaExpected::Revision(history[prior].revision())
            });
        assert_eq!(
            store
                .compare_and_swap(expected, record.clone())
                .await
                .expect("runtime history transition should persist"),
            WorkloadSagaCommit::Applied
        );
    }
}

fn runtime_withdrawal_history(
    bundle: &nimbus_network::NetworkCapabilityBundle,
) -> Vec<WorkloadSagaRecord> {
    runtime_withdrawal_history_for_listeners(bundle, &["http"])
}

fn runtime_withdrawal_history_for_listeners(
    bundle: &nimbus_network::NetworkCapabilityBundle,
    listener_names: &[&str],
) -> Vec<WorkloadSagaRecord> {
    let tenant_id = TenantId::new("tenant-runtime-ingress").expect("runtime tenant should parse");
    let key = nimbus_workloads::WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new("runtime-ingress").expect("runtime workload should parse"),
    );
    let running = runtime_intent_for_listeners(
        &key,
        1,
        DesiredWorkloadState::Running,
        WorkloadPublicationIntent::PublishWhenReady,
        bundle,
        listener_names,
    );
    let mut history = vec![
        WorkloadSagaRecord::new(key.clone(), running).expect("runtime saga should initialize"),
    ];
    while history
        .last()
        .expect("runtime provision history should remain non-empty")
        .phase()
        != WorkloadSagaPhase::Observed
    {
        extend_runtime_provision_step(&mut history);
    }
    let observed = history
        .last()
        .expect("runtime history should reach observed");
    let stopped = runtime_intent(
        &key,
        2,
        DesiredWorkloadState::Stopped,
        WorkloadPublicationIntent::Withheld,
        bundle,
    );
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = observed
        .apply_intent(stopped)
        .expect("stopped successor should start teardown")
    else {
        panic!("stopped successor must change durable state");
    };
    history.push(*withdrawal);
    history
}

fn runtime_intent(
    key: &nimbus_workloads::WorkloadSagaKey,
    generation: u64,
    desired_state: DesiredWorkloadState,
    publication: WorkloadPublicationIntent,
    bundle: &nimbus_network::NetworkCapabilityBundle,
) -> WorkloadSagaIntent {
    runtime_intent_for_listeners(
        key,
        generation,
        desired_state,
        publication,
        bundle,
        &["http"],
    )
}

fn runtime_intent_for_listeners(
    key: &nimbus_workloads::WorkloadSagaKey,
    generation: u64,
    desired_state: DesiredWorkloadState,
    publication: WorkloadPublicationIntent,
    bundle: &nimbus_network::NetworkCapabilityBundle,
    listener_names: &[&str],
) -> WorkloadSagaIntent {
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"runtime-ingress-{generation}"}}"#),
    )
    .expect("runtime executable should validate");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox("runtime-ingress", "fixture")
            .expect("runtime source identity should validate"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new(format!("runtime-v{generation}"))
            .expect("runtime source version should validate"),
        executable.content_digest(),
        runtime_attachment_provider_id(),
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("runtime-execution"),
    )
    .expect("runtime source evidence should validate");
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        nimbus_workloads::WorkloadGeneration::new(generation),
        executable,
        source,
        WorkloadNetworkIntent::new(if listener_names == ["http"] {
            runtime_compiled_plan(key.tenant_id(), generation, publication, bundle)
        } else {
            runtime_compiled_plan_for_listeners(
                key.tenant_id(),
                generation,
                publication,
                bundle,
                listener_names,
            )
        }),
        if desired_state == DesiredWorkloadState::Running {
            WorkloadActivationIntent::ActivateWhenAttached
        } else {
            WorkloadActivationIntent::PrepareOnly
        },
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("runtime decision ID should validate"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("runtime workload UID should validate"),
            NodeIdentity::new("node-runtime-ingress").expect("runtime node should validate"),
        ),
    )
    .expect("runtime workload intent should validate")
}

fn runtime_compiled_plan(
    tenant_id: &TenantId,
    generation: u64,
    publication: WorkloadPublicationIntent,
    bundle: &nimbus_network::NetworkCapabilityBundle,
) -> CompiledWorkloadNetworkPlan {
    runtime_compiled_plan_for_listeners(tenant_id, generation, publication, bundle, &["http"])
}

fn runtime_compiled_plan_for_listeners(
    tenant_id: &TenantId,
    generation: u64,
    publication: WorkloadPublicationIntent,
    bundle: &nimbus_network::NetworkCapabilityBundle,
    listener_names: &[&str],
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        "runtime-ingress",
        NetworkResourceGeneration::new(generation),
    )
    .expect("runtime network identity should validate");
    let attachment =
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []);
    let endpoint = NetworkEndpointCapabilitySet::new(
        [NetworkAddressFamily::Ipv4],
        [nimbus_network::NetworkBindRealmKind::Host],
        [NetworkExposure::Loopback],
        [PortProtocol::Tcp],
        [NetworkPortAssignmentMode::ProviderAssigned],
    );
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let requirements = NetworkCapabilityRequirements::new(
        attachment,
        endpoint,
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let (selection, evidence, listeners) =
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            let listeners = listener_names
                .iter()
                .map(|name| {
                    WorkloadNetworkListenerBlueprint::new(
                        &identity,
                        *name,
                        EndpointProtocol::Http,
                        Ipv4Addr::LOCALHOST.into(),
                        nimbus_workloads::WorkloadNetworkPortRequestMode::ProviderAssigned,
                        WorkloadNetworkEndpointSemantics::new(
                            WorkloadNetworkForwardingBehavior::None,
                            NetworkTlsBehavior::Disabled,
                        ),
                        None,
                    )
                    .expect("runtime listener blueprint should validate")
                })
                .collect();
            (
                Some(bundle.selection()),
                Some(bundle.selection_evidence()),
                listeners,
            )
        } else {
            (None, None, Vec::new())
        };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        selection,
        evidence,
        None,
        [],
        listeners,
        [],
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            WorkloadActivationIntent::ActivateWhenAttached
        } else {
            WorkloadActivationIntent::PrepareOnly
        },
        publication,
    )
    .expect("runtime network content should validate");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("runtime compiled plan should validate")
}

fn runtime_network_bundle() -> nimbus_network::NetworkCapabilityBundle {
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    nimbus_network::NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            runtime_attachment_provider_id(),
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            [NetworkAddressFamily::Ipv4],
            lifecycle.clone(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        NetworkIngressProviderRegistration::new(
            nimbus_owned_local_ingress_provider_id(),
            NetworkEndpointCapabilitySet::new(
                [NetworkAddressFamily::Ipv4],
                [nimbus_network::NetworkBindRealmKind::Host],
                [NetworkExposure::Loopback],
                [PortProtocol::Tcp],
                [NetworkPortAssignmentMode::ProviderAssigned],
            ),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            lifecycle,
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
    )
}

fn runtime_attachment_provider_id() -> NetworkProviderId {
    NetworkProviderId::for_registration_key("runtime-attachment")
}

fn extend_runtime_provision_step(history: &mut Vec<WorkloadSagaRecord>) {
    let current = history
        .last()
        .expect("runtime provision history should remain non-empty");
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(current).expect("runtime phase should reduce")
    else {
        panic!("runtime provision phase should produce a candidate");
    };
    let mut candidate = proposed.into_candidate();
    history.push(candidate.clone());
    while let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
        candidate.provision_disposition()
    {
        let attempt = claim.attempt();
        let result = WorkloadProvisionEffectResult::Succeeded {
            attempt_id: attempt.attempt_id().clone(),
            evidence: runtime_provision_success(attempt),
        };
        let WorkloadProvisionDecision::Proposed(proposed) =
            WorkloadProvisionDecision::reduce(&candidate, result)
                .expect("runtime provision success should reduce")
        else {
            panic!("runtime provision success should produce a candidate");
        };
        candidate = proposed.into_candidate();
        history.push(candidate.clone());
    }
}

fn runtime_provision_success(
    attempt: &nimbus_workloads::WorkloadProvisionAttempt,
) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("{:?}", attempt.step()));
    match (attempt.step(), attempt.subjects()) {
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
        _ => panic!("runtime provision attempt should retain matching subjects"),
    }
}

fn install_runtime_live_batch(
    adapter: &ServerIngressPublicationAdapter,
    network_authority: &LocalNetworkAuthority,
    observed: &WorkloadSagaRecord,
) {
    let reference = observed
        .phase_detail()
        .references()
        .publication()
        .expect("observed runtime record should retain publication")
        .clone();
    let listeners = observed
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .listeners();
    assert!(
        !listeners.is_empty(),
        "runtime plan should retain listeners"
    );
    let requests = listeners
        .iter()
        .map(|listener| {
            PortLeaseRequest::new(
                listener.port_lease_id().clone(),
                NetworkResourceId::from(listener.listener_id().clone()),
                Some(observed.key().tenant_id().clone()),
                PortLeaseFence::new(reference.network().generation(), NetworkLeaseEpoch::new(1)),
                PortLeaseAccounting::TenantPublished,
                PortPublicationIntent::host(listener.desired_host_address()),
                PortBindingSpec::new(
                    PortProtocol::Tcp,
                    PortBindRealm::Host,
                    PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
                    PortExposure::Loopback,
                    PortRequestMode::ProviderAssigned,
                ),
            )
            .with_plan_id(reference.network().plan_id().clone())
        })
        .collect::<Vec<_>>();
    let claim = reservation_claim("runtime-final-ingress");
    network_authority
        .port_leases()
        .reserve_batch_for_coordinator(requests.clone(), &claim)
        .expect("runtime launch owner should reserve every exact listener");
    let plan_members = adapter
        .listeners
        .authenticate_workload_ingress_plan(
            reference.network().plan_id(),
            observed.key().tenant_id(),
            reference.network().generation(),
            &requests,
            &claim,
        )
        .expect("runtime server should authenticate the complete plan");
    let routes = listeners
        .iter()
        .zip(requests)
        .map(|(listener, request)| {
            let prepared = adapter
                .listeners
                .prepare_workload_ingress(Some(&plan_members), request.clone(), &claim)
                .expect("runtime server should claim the exact listener");
            let socket = TcpListener::bind(
                prepared
                    .bind_addr()
                    .expect("runtime listener address should resolve"),
            )
            .expect("runtime listener should bind");
            RunningIngressRoute::start(
                ExpectedRoute {
                    listener_id: listener.listener_id().clone(),
                    request,
                    upstream: (Ipv4Addr::LOCALHOST, 9).into(),
                },
                prepared
                    .adopt_std(socket)
                    .expect("runtime listener should adopt its exact lease"),
                DEFAULT_MAX_ACTIVE_CONNECTIONS,
            )
            .expect("runtime listener worker should start")
        })
        .collect::<Vec<_>>();
    let selection = observed
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .capability_selection_evidence()
        .expect("runtime plan should retain selected provider evidence");
    adapter
        .running
        .lock()
        .expect("runtime live registry should remain healthy")
        .insert(
            PublicationKey {
                saga_id: observed.saga_id().as_str().to_owned(),
                attempt_id: "runtime-publication-attempt".to_owned(),
                execution_id: reference.execution().execution_id().as_str().to_owned(),
                generation: reference.network().generation().as_u64(),
                network_plan_digest: reference.network().digest().to_string(),
            },
            RunningIngressBatch {
                execution_id: reference.execution().execution_id().as_str().to_owned(),
                tenant_id: observed.key().tenant_id().clone(),
                plan_id: reference.network().plan_id().clone(),
                generation: reference.network().generation(),
                attachment_id: NetworkAttachmentId::for_workload_attachment(
                    "tenant-runtime-ingress/runtime-ingress",
                    "private",
                ),
                plan_members,
                routes,
                publication: PublishedIngressAuthority::new(
                    reference,
                    selection.source_digest(),
                    observed.active_intent().source().source_digest(),
                ),
                final_phase: FinalIngressPhase::Published,
            },
        );
}

struct LiveObservationFixture {
    adapter: Arc<ServerIngressPublicationAdapter>,
    network_authority: LocalNetworkAuthority,
    query: LiveIngressObservationQuery,
    expected_lifetimes: BTreeMap<PublishedEndpointId, PortLeaseLifetime>,
    state_root: tempfile::TempDir,
    _process_authority_guard: tokio::sync::MutexGuard<'static, ()>,
}

fn live_workload_request(
    tenant_id: &nimbus_core::TenantId,
    plan_id: &NetworkPlanId,
    listener_name: &str,
) -> PortLeaseRequest {
    let listener = ListenerId::for_tenant_workload_listener(tenant_id, "workload-a", listener_name);
    PortLeaseRequest::new(
        PortLeaseId::for_listener(&listener),
        NetworkResourceId::from(listener),
        Some(tenant_id.clone()),
        PortLeaseFence::new(NetworkResourceGeneration::new(7), NetworkLeaseEpoch::new(1)),
        PortLeaseAccounting::TenantPublished,
        PortPublicationIntent::host(Ipv4Addr::LOCALHOST.into()),
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
            PortExposure::Loopback,
            PortRequestMode::ProviderAssigned,
        ),
    )
    .with_plan_id(plan_id.clone())
}

fn live_observation_fixture(listener_names: &[&str]) -> LiveObservationFixture {
    let process_authority_guard = process_network_authority_test_guard();
    let state_root = tempfile::tempdir().expect("fixture root should exist");
    let bootstrap = LocalNetworkManager::bootstrap(state_root.path())
        .expect("fixture network authority should bootstrap");
    let manager = bootstrap
        .freeze(NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"));
    let network_authority = manager.authority();
    let adapter = Arc::new(
        ServerIngressPublicationAdapter::new(
            Arc::new(AbsentContainerIngressSource),
            network_authority.clone(),
        )
        .expect("server ingress journal should open"),
    );
    let tenant_id = nimbus_core::TenantId::new("tenant-a").expect("fixture tenant should parse");
    let plan_id = NetworkPlanId::for_tenant_workload_plan(&tenant_id, "workload-a");
    let plan_digest = NetworkPlanDigest::from_bytes([0x4a; 32]);
    let generation = NetworkResourceGeneration::new(7);
    let claim = reservation_claim("server-ingress-observation");
    let mut routes = Vec::new();
    let mut listeners = BTreeMap::new();
    let mut expected_lifetimes = BTreeMap::new();

    // Deliberately reverse construction order. Observation must canonicalize
    // by stable endpoint identity rather than preserving provider order.
    let planned = listener_names
        .iter()
        .rev()
        .map(|listener_name| {
            let listener_id =
                ListenerId::for_tenant_workload_listener(&tenant_id, "workload-a", listener_name);
            let endpoint_id =
                PublishedEndpointId::for_workload_endpoint("tenant-a/workload-a", listener_name);
            let request = live_workload_request(&tenant_id, &plan_id, listener_name);
            (listener_id, endpoint_id, request)
        })
        .collect::<Vec<_>>();
    let requested_plan = planned
        .iter()
        .map(|(_, _, request)| request.clone())
        .collect::<Vec<_>>();
    network_authority
        .port_leases()
        .reserve_batch_for_coordinator(requested_plan.clone(), &claim)
        .expect("launch owner should atomically reserve the complete listener plan");
    let plan_members = adapter
        .listeners
        .authenticate_workload_ingress_plan(
            &plan_id,
            &tenant_id,
            generation,
            &requested_plan,
            &claim,
        )
        .expect("server publisher should authenticate complete durable plan membership");

    for (listener_id, endpoint_id, request) in planned {
        let prepared = adapter
            .listeners
            .prepare_workload_ingress(Some(&plan_members), request.clone(), &claim)
            .expect("server publisher should claim the launch reservation");
        let listener = TcpListener::bind(
            prepared
                .bind_addr()
                .expect("authorized bind address should resolve"),
        )
        .expect("fixture publication listener should bind");
        let adopted = prepared
            .adopt_std(listener)
            .expect("fixture listener should activate the exact lease");
        let route = RunningIngressRoute::start(
            ExpectedRoute {
                listener_id: listener_id.clone(),
                request: request.clone(),
                upstream: (Ipv4Addr::LOCALHOST, 9).into(),
            },
            adopted,
            DEFAULT_MAX_ACTIVE_CONNECTIONS,
        )
        .expect("fixture ingress worker should start");
        let evidence = route
            .lease
            .as_ref()
            .and_then(ActiveServerListenerLease::observation_evidence)
            .expect("live fixture lease should expose stripped observation evidence");
        expected_lifetimes.insert(endpoint_id.clone(), evidence.lifetime());
        listeners.insert(
            listener_id.clone(),
            LiveIngressListenerExpectation {
                endpoint_id,
                listener_id,
                port_lease_id: request.lease_id().clone(),
                desired_host_address: Ipv4Addr::LOCALHOST.into(),
            },
        );
        routes.push(route);
    }

    let saga_id = "fixture-workload-saga".to_owned();
    adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .insert(
            PublicationKey {
                saga_id: saga_id.clone(),
                attempt_id: "fixture-publication-attempt".to_owned(),
                execution_id: "execution-workload-a".to_owned(),
                generation: generation.as_u64(),
                network_plan_digest: plan_digest.to_string(),
            },
            RunningIngressBatch {
                execution_id: "execution-workload-a".to_owned(),
                tenant_id: tenant_id.clone(),
                plan_id: plan_id.clone(),
                generation,
                attachment_id: NetworkAttachmentId::for_workload_attachment(
                    "tenant-a/workload-a",
                    "private",
                ),
                plan_members,
                routes,
                publication: PublishedIngressAuthority::direct_fixture(),
                final_phase: FinalIngressPhase::Published,
            },
        );

    LiveObservationFixture {
        adapter,
        network_authority,
        query: LiveIngressObservationQuery {
            saga_id,
            execution_id: "execution-workload-a".to_owned(),
            attempt_id: "fixture-publication-attempt".to_owned(),
            tenant_id,
            plan_id,
            plan_digest,
            generation,
            listeners,
        },
        expected_lifetimes,
        state_root,
        _process_authority_guard: process_authority_guard,
    }
}

fn snapshot_regular_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, snapshot: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(path)
            .expect("fixture state directory should list")
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture state entries should resolve");
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let kind = entry.file_type().expect("fixture file type should resolve");
            if kind.is_dir() {
                visit(root, &path, snapshot);
            } else if kind.is_file() {
                snapshot.insert(
                    path.strip_prefix(root)
                        .expect("fixture path should remain below root")
                        .to_path_buf(),
                    fs::read(path).expect("fixture state file should read"),
                );
            }
        }
    }

    let mut snapshot = BTreeMap::new();
    visit(root, root, &mut snapshot);
    snapshot
}

fn restart_publication_for(
    key: &PublicationKey,
    batch: &RunningIngressBatch,
) -> ValidatedRestartPublication {
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let network_plan = NetworkPlan::new(
        batch.plan_id.clone(),
        batch.generation,
        NetworkPlanContentDigest::sha256(b"restart-listener-retention-fixture"),
        requirements,
    );
    let endpoint_identities = batch.routes.iter().enumerate().map(|(ordinal, route)| {
        nimbus_sandbox::SandboxProvisionEndpointIdentity::new(
            route.expected.listener_id.clone(),
            PublishedEndpointId::for_workload_endpoint(
                &key.saga_id,
                &format!("listener-{ordinal}"),
            ),
        )
    });
    let listeners = batch.routes.iter().enumerate().map(|(ordinal, route)| {
        nimbus_sandbox::SandboxProvisionListener::new(
            PublishedEndpointId::for_workload_endpoint(
                &key.saga_id,
                &format!("listener-{ordinal}"),
            ),
            route.expected.listener_id.clone(),
            nimbus_sandbox::SandboxPortBinding::tcp(format!("listener-{ordinal}"), 0, 9),
            route.expected.request.clone(),
        )
    });
    let network_plan = SandboxProvisionNetworkPlan::new(
        network_plan,
        batch.tenant_id.clone(),
        batch.generation,
        batch.attachment_id.clone(),
        endpoint_identities,
        listeners,
        [],
    )
    .expect("restart fixture network plan should validate");
    let source_attempt = nimbus_sandbox::SandboxExecutionAttemptId::new(key.attempt_id.clone())
        .expect("source attempt should validate");
    let target_attempt = nimbus_sandbox::SandboxExecutionAttemptId::new("restart-target")
        .expect("target attempt should validate");
    let attempt_fence =
        nimbus_sandbox::SandboxRestartAttemptFence::new(source_attempt, target_attempt, 1)
            .expect("restart attempt fence should validate");
    ValidatedRestartPublication {
        source_key: key.clone(),
        target_key: PublicationKey {
            saga_id: key.saga_id.clone(),
            attempt_id: attempt_fence.attempt_id().as_str().to_owned(),
            execution_id: key.execution_id.clone(),
            generation: key.generation,
            network_plan_digest: key.network_plan_digest.clone(),
        },
        sandbox_id: nimbus_sandbox::SandboxId::new(key.execution_id.clone()),
        attempt_fence,
        network_plan,
        publication: PublishedIngressAuthority::direct_fixture(),
    }
}

fn expect_present(
    observation: WorkloadProviderObservation<Vec<WorkloadObservedIngressEndpoint>>,
) -> Vec<WorkloadObservedIngressEndpoint> {
    match observation {
        WorkloadProviderObservation::Present(endpoints) => endpoints,
        other => panic!("expected present ingress evidence, got {other:?}"),
    }
}

#[test]
fn live_observation_returns_canonical_provider_assigned_witnesses_without_mutation() {
    let fixture = live_observation_fixture(&["http", "admin"]);
    let before_files = snapshot_regular_files(fixture.state_root.path());
    let before_leases = fixture
        .network_authority
        .port_leases()
        .list()
        .expect("fixture leases should list");

    let first = fixture.adapter.observe_live_publication(&fixture.query);
    let replay = fixture.adapter.observe_live_publication(&fixture.query);
    assert_eq!(
        replay, first,
        "replay must return byte-identical value state"
    );
    let endpoints = expect_present(first.clone());
    assert_eq!(endpoints.len(), 2);
    assert!(
        endpoints
            .windows(2)
            .all(|pair| pair[0].endpoint_id() < pair[1].endpoint_id()),
        "provider order must be canonicalized by stable endpoint identity"
    );
    for endpoint in &endpoints {
        let expected = fixture
            .query
            .listeners
            .values()
            .find(|expected| expected.endpoint_id == *endpoint.endpoint_id())
            .expect("every observed endpoint should be an authenticated member");
        let binding = endpoint.binding();
        assert_eq!(binding.plan_id(), &fixture.query.plan_id);
        assert_eq!(binding.plan_digest(), fixture.query.plan_digest);
        assert_eq!(binding.generation(), fixture.query.generation);
        assert_eq!(binding.listener_id(), &expected.listener_id);
        assert_eq!(binding.port_lease_id(), &expected.port_lease_id);
        assert_eq!(binding.lifetime(), binding.binding_lifetime());
        assert_eq!(
            fixture.expected_lifetimes.get(endpoint.endpoint_id()),
            Some(&binding.lifetime())
        );
        assert_eq!(
            binding.provenance(),
            PortBindingProvenance::ProviderAssigned
        );
        assert_eq!(
            binding.bound_endpoint().port().get(),
            endpoint.published_address().port()
        );
        assert_eq!(
            binding.bound_endpoint().target().specific_address(),
            Some(endpoint.published_address().ip())
        );
    }

    let concurrent = std::thread::scope(|scope| {
        (0..8)
            .map(|_| scope.spawn(|| fixture.adapter.observe_live_publication(&fixture.query)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("observation worker should not panic"))
            .collect::<Vec<_>>()
    });
    assert!(concurrent.iter().all(|observation| observation == &first));
    assert_eq!(
        fixture
            .network_authority
            .port_leases()
            .list()
            .expect("fixture leases should remain readable"),
        before_leases,
        "observation must not mutate durable lease authority"
    );
    assert_eq!(
        snapshot_regular_files(fixture.state_root.path()),
        before_files,
        "replay and concurrent observation must leave journal and lease bytes unchanged"
    );
}

#[test]
fn final_withdrawal_closes_routes_joins_workers_and_releases_exact_leases() {
    let fixture = live_observation_fixture(&["http", "admin"]);
    let (_, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one live publication batch");
    let addresses = batch
        .routes
        .iter()
        .map(|route| route.bound_addr)
        .collect::<Vec<_>>();
    let requests = batch
        .routes
        .iter()
        .map(|route| route.expected.request.clone())
        .collect::<Vec<_>>();

    batch
        .stop_and_release_for_final_withdrawal()
        .expect("exact final withdrawal should settle every route and lease");

    for (request, address) in requests.iter().zip(addresses) {
        let record = fixture
            .network_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("released listener should inspect")
            .expect("released listener history should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Released);
        drop(
            TcpListener::bind(address)
                .expect("the old address must be reusable only after terminal release"),
        );
    }
}

#[test]
fn final_withdrawal_settlement_failure_blocks_progress_and_preserves_fences() {
    let fixture = live_observation_fixture(&["http", "admin"]);
    let (_, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one live publication batch");
    let requests = batch
        .routes
        .iter()
        .map(|route| route.expected.request.clone())
        .collect::<Vec<_>>();
    batch.routes[0].inject_final_join_failure_for_test();

    batch
        .stop_and_release_for_final_withdrawal()
        .expect_err("one ambiguous join must block final withdrawal success");

    for request in requests {
        let record = fixture
            .network_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("ambiguous listener should inspect")
            .expect("ambiguous listener must remain durably fenced");
        assert_eq!(record.phase(), PortLeasePhase::Withdrawing);
        assert!(record.active_lifetime().is_some());
    }
}

#[test]
fn final_withdrawal_missing_owner_stops_siblings_and_preserves_every_fence() {
    let fixture = live_observation_fixture(&["lost", "sibling"]);
    let (_, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one live publication batch");
    let requests = batch
        .routes
        .iter()
        .map(|route| route.expected.request.clone())
        .collect::<Vec<_>>();
    let sibling_address = batch.routes[1].bound_addr;
    batch.routes[0].abandon_final_worker_owner_for_test();

    batch
        .stop_and_release_for_final_withdrawal()
        .expect_err("a lost terminal owner must make final withdrawal ambiguous");

    assert!(
        batch.routes[1].stop.load(Ordering::Acquire),
        "the intact sibling must still receive its terminal stop signal"
    );
    drop(
        TcpListener::bind(sibling_address)
            .expect("the intact sibling listener must close even when another owner is lost"),
    );
    for request in requests {
        let record = fixture
            .network_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("ambiguous listener should inspect")
            .expect("ambiguous listener must retain durable recovery evidence");
        assert_eq!(record.phase(), PortLeasePhase::Withdrawing);
        assert!(record.active_lifetime().is_some());
    }
}

#[test]
fn final_withdrawal_recovers_dead_process_bound_listeners_without_rebind() {
    let fixture = live_observation_fixture(&["http", "admin"]);
    let (_, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one live publication batch");
    let plan_members = batch.plan_members.clone();
    let requests = batch
        .routes
        .iter()
        .map(|route| route.expected.request.clone())
        .collect::<Vec<_>>();
    let addresses = batch
        .routes
        .iter()
        .map(|route| route.bound_addr)
        .collect::<Vec<_>>();
    for route in &mut batch.routes {
        drop(
            route
                .take_for_restart()
                .expect("fixture route should retain one process-bound listener"),
        );
    }
    drop(batch);

    recover_dead_process_bound_server_listeners_for_final_withdrawal(
        &fixture.network_authority,
        &plan_members,
        &requests,
    )
    .expect("dead process-bound listeners should release without rebind");

    for (request, address) in requests.iter().zip(addresses) {
        let record = fixture
            .network_authority
            .port_leases()
            .inspect(request.lease_id())
            .expect("recovered listener should inspect")
            .expect("recovered listener history should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Released);
        drop(
            TcpListener::bind(address)
                .expect("owner-death recovery must not retain or rebind the old address"),
        );
    }
}

#[test]
fn restart_without_live_listener_ownership_remains_in_progress_and_effect_free() {
    let fixture = live_observation_fixture(&["http"]);
    let restarted = ServerIngressPublicationAdapter::new(
        Arc::new(AbsentContainerIngressSource),
        fixture.network_authority.clone(),
    )
    .expect("restart fixture should reopen the existing phase journal");
    let before = snapshot_regular_files(fixture.state_root.path());

    assert_eq!(
        restarted.observe_live_publication(&fixture.query),
        WorkloadProviderObservation::InProgress
    );
    assert_eq!(snapshot_regular_files(fixture.state_root.path()), before);
}

#[test]
fn restart_withdrawal_joins_listener_and_rebinds_the_same_retained_port() {
    let fixture = live_observation_fixture(&["http"]);
    let (key, batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one exact publication");
    let request = batch.routes[0].expected.request.clone();
    let expected = batch.routes[0].expected.clone();
    let original_port = batch.routes[0].bound_addr.port();
    let plan_members = batch.plan_members.clone();

    let evidence = batch
        .stop_and_retain_for_restart()
        .expect("restart withdrawal should stop, join, and retain the complete batch");
    assert!(String::from_utf8_lossy(&evidence).contains("restart_retained="));
    let retained = fixture
        .network_authority
        .port_leases()
        .inspect(request.lease_id())
        .expect("retained lease should inspect")
        .expect("retained lease should remain durable");
    assert_eq!(retained.phase(), nimbus_network::PortLeasePhase::Reserved);
    assert_eq!(
        retained
            .confirmed_stopped_binding()
            .expect("retained lease should carry confirmed-stop evidence")
            .actual_port()
            .get(),
        original_port
    );

    let prepared = fixture
        .adapter
        .listeners
        .prepare_workload_ingress(
            Some(&plan_members),
            request.clone(),
            &reservation_claim("server-ingress-observation"),
        )
        .expect("target attempt should claim the retained exact-port rebind");
    let bind_addr = prepared
        .bind_addr()
        .expect("retained rebind address should resolve");
    assert_eq!(bind_addr.port(), original_port);
    let listener = TcpListener::bind(bind_addr).expect("retained exact port should rebind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("target listener should adopt the retained lease");
    let route = RunningIngressRoute::start(expected, adopted, DEFAULT_MAX_ACTIVE_CONNECTIONS)
        .expect("target attempt ingress route should start");
    assert_eq!(route.bound_addr.port(), original_port);
    assert!(route.is_healthy());
    drop(route);
    assert!(
        !fixture
            .adapter
            .running
            .lock()
            .expect("fixture registry lock should remain healthy")
            .contains_key(&key),
        "the source attempt must stay withdrawn"
    );
}

#[test]
fn restart_withdrawal_inspection_requires_durable_retention_and_never_recovers() {
    let fixture = live_observation_fixture(&["http"]);
    let (key, mut batch) = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .pop_first()
        .expect("fixture should retain one exact publication");
    let validated = restart_publication_for(&key, &batch);
    for route in &mut batch.routes {
        drop(
            route
                .take_for_restart()
                .expect("fixture route should retain its listener effect"),
        );
    }
    drop(batch);

    let before = snapshot_regular_files(fixture.state_root.path());
    let inspected = fixture.adapter.inspect_restart_withdrawal(&validated);
    assert!(
        matches!(inspected, ProviderRestartEffectObservation::Absent { .. }),
        "active durable state without a live owner is not retained withdrawal evidence"
    );
    assert_eq!(snapshot_regular_files(fixture.state_root.path()), before);

    let recovered = fixture.adapter.withdraw_restart_publication(&validated);
    assert!(
        matches!(
            recovered,
            ProviderRestartEffectObservation::Succeeded { .. }
        ),
        "execute-time recovery must durably retain the dead listener"
    );
    let after_recovery = snapshot_regular_files(fixture.state_root.path());
    assert!(
        matches!(
            fixture.adapter.inspect_restart_withdrawal(&validated),
            ProviderRestartEffectObservation::Succeeded { .. }
        ),
        "exact retained records must be sufficient withdrawal evidence"
    );
    assert_eq!(
        snapshot_regular_files(fixture.state_root.path()),
        after_recovery
    );
}

#[test]
fn crossed_or_unhealthy_live_ingress_evidence_fails_closed() {
    let fixture = live_observation_fixture(&["http"]);
    let mut cases = Vec::new();

    let mut wrong_plan = fixture.query.clone();
    wrong_plan.plan_id =
        NetworkPlanId::for_tenant_workload_plan(&fixture.query.tenant_id, "replacement-workload");
    cases.push(wrong_plan);
    let mut wrong_digest = fixture.query.clone();
    wrong_digest.plan_digest = NetworkPlanDigest::from_bytes([0x9b; 32]);
    cases.push(wrong_digest);
    let mut wrong_execution = fixture.query.clone();
    wrong_execution.execution_id = "execution-crossed".to_owned();
    cases.push(wrong_execution);
    let mut wrong_generation = fixture.query.clone();
    wrong_generation.generation = NetworkResourceGeneration::new(8);
    cases.push(wrong_generation);
    let mut wrong_tenant = fixture.query.clone();
    wrong_tenant.tenant_id =
        nimbus_core::TenantId::new("tenant-b").expect("crossed tenant should parse");
    cases.push(wrong_tenant);
    let mut wrong_lease = fixture.query.clone();
    let expectation = wrong_lease
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener");
    expectation.port_lease_id =
        PortLeaseId::for_listener(&ListenerId::for_tenant_workload_listener(
            &fixture.query.tenant_id,
            "workload-a",
            "crossed",
        ));
    cases.push(wrong_lease);
    let mut wrong_listener = fixture.query.clone();
    wrong_listener
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener")
        .listener_id =
        ListenerId::for_tenant_workload_listener(&fixture.query.tenant_id, "workload-a", "crossed");
    cases.push(wrong_listener);
    let mut wrong_host = fixture.query.clone();
    wrong_host
        .listeners
        .values_mut()
        .next()
        .expect("fixture should carry one listener")
        .desired_host_address = Ipv4Addr::new(127, 0, 0, 2).into();
    cases.push(wrong_host);

    for crossed in cases {
        assert_eq!(
            fixture.adapter.observe_live_publication(&crossed),
            WorkloadProviderObservation::Ambiguous,
            "crossed stable identity, fence, or desired bind must fail closed"
        );
    }

    {
        let running = fixture
            .adapter
            .running
            .lock()
            .expect("fixture registry lock should remain healthy");
        let route = &running
            .values()
            .next()
            .expect("fixture batch should remain live")
            .routes[0];
        route.failed.store(true, Ordering::Release);
    }
    assert_eq!(
        fixture.adapter.observe_live_publication(&fixture.query),
        WorkloadProviderObservation::Ambiguous
    );
}

#[test]
fn live_listener_batch_never_regresses_to_absent_when_its_source_temporarily_disappears() {
    let fixture = live_observation_fixture(&["http"]);
    let key = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy")
        .keys()
        .next()
        .expect("fixture should retain one exact publication")
        .clone();

    let inspected = fixture.adapter.inspect_with_source(
        &key,
        Err(ProviderProvisionEffectObservation::Absent {
            evidence: b"source temporarily absent".to_vec(),
        }),
        true,
    );
    assert!(
        matches!(
            inspected,
            ProviderProvisionEffectObservation::Succeeded { .. }
        ),
        "a healthy owned listener is durable positive evidence and cannot authorize a duplicate retry"
    );
}

#[test]
fn live_listener_batch_preserves_ambiguous_source_on_publication_replay() {
    let fixture = live_observation_fixture(&["http"]);
    let running = fixture
        .adapter
        .running
        .lock()
        .expect("fixture registry lock should remain healthy");
    let (key, batch) = running
        .iter()
        .next()
        .expect("fixture should retain one exact publication");
    let observation = classify_existing_publication(
        batch,
        &key.execution_id,
        Err(ProviderProvisionEffectObservation::Ambiguous {
            evidence: b"private-route source temporarily unavailable".to_vec(),
        }),
    );
    assert!(
        matches!(
            observation,
            ProviderProvisionEffectObservation::Ambiguous { ref evidence }
                if evidence == b"private-route source temporarily unavailable"
        ),
        "a transient source read cannot convert a live exact listener into terminal failure"
    );
}

#[test]
fn transparent_tcp_route_forwards_bytes_and_releases_its_exact_lease() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let request = workload_request("http");
    let claim = reservation_claim("server-ingress-forwarding");
    let port_authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should open");
    port_authority
        .reserve_for_coordinator(request.clone(), &claim)
        .expect("launch owner should reserve the exact request");

    let upstream =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("fixture upstream listener should bind");
    let upstream_addr = upstream
        .local_addr()
        .expect("fixture upstream address should resolve");
    let upstream_worker = std::thread::spawn(move || {
        let (mut stream, _) = upstream.accept().expect("proxy should reach upstream");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("upstream timeout should configure");
        let mut request = [0_u8; 4];
        stream
            .read_exact(&mut request)
            .expect("upstream should receive the exact request");
        assert_eq!(&request, b"ping");
        stream
            .write_all(b"pong")
            .expect("upstream should return the exact response");
    });

    let authority = ServerListenerLeaseAuthority::reconstruct_direct(root.path())
        .expect("listener authority should reconstruct");
    let prepared = authority
        .prepare_workload_ingress(None, request.clone(), &claim)
        .expect("publication should claim the launch reservation");
    let listener = TcpListener::bind(
        prepared
            .bind_addr()
            .expect("authorized bind address should resolve"),
    )
    .expect("publication listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("publication listener should activate the exact lease");
    let route = RunningIngressRoute::start(
        ExpectedRoute {
            listener_id: ListenerId::for_workload_listener("tenant-a/workload-a", "http"),
            request: request.clone(),
            upstream: upstream_addr,
        },
        adopted,
        DEFAULT_MAX_ACTIVE_CONNECTIONS,
    )
    .expect("transparent ingress route should start");
    assert!(route.is_healthy());
    let published_addr = route.bound_addr;

    let mut client = TcpStream::connect(published_addr)
        .expect("client should connect through the published route");
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("client timeout should configure");
    client
        .write_all(b"ping")
        .expect("client request should reach the route");
    client
        .shutdown(Shutdown::Write)
        .expect("client write half should close");
    let mut response = [0_u8; 4];
    client
        .read_exact(&mut response)
        .expect("client should receive the upstream response");
    assert_eq!(&response, b"pong");
    upstream_worker
        .join()
        .expect("upstream worker should finish cleanly");

    drop(route);
    let settled = port_authority
        .inspect(request.lease_id())
        .expect("settled lease should inspect")
        .expect("released lease evidence should remain observable");
    assert_eq!(settled.phase(), nimbus_network::PortLeasePhase::Released);
    assert!(settled.reservation_claim().is_none());
    assert!(settled.bind_claim().is_none());
    let binding = settled
        .binding()
        .expect("released authority should retain observed binding history");
    assert_eq!(binding.actual_port().get(), published_addr.port());
    assert_eq!(
        binding.provenance(),
        nimbus_network::PortBindingProvenance::ProviderAssigned
    );
}

#[test]
fn ingress_route_bounds_tracks_and_joins_connection_workers_before_lease_settlement() {
    let root = tempfile::tempdir().expect("fixture root should exist");
    let request = workload_request("bounded");
    let claim = reservation_claim("server-ingress-bounded-workers");
    let port_authority =
        LocalPortLeaseAuthority::open(root.path()).expect("port authority should open");
    port_authority
        .reserve_for_coordinator(request.clone(), &claim)
        .expect("launch owner should reserve the exact request");
    let upstream =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("fixture upstream should bind");
    let upstream_addr = upstream
        .local_addr()
        .expect("upstream address should resolve");
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let upstream_worker = std::thread::spawn(move || {
        let (mut stream, _) = upstream
            .accept()
            .expect("first proxy should reach upstream");
        accepted_tx
            .send(())
            .expect("accept signal receiver should remain open");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("upstream timeout should configure");
        let mut byte = [0_u8; 1];
        let _ = stream.read(&mut byte);
    });

    let authority = ServerListenerLeaseAuthority::reconstruct_direct(root.path())
        .expect("listener authority should reconstruct");
    let prepared = authority
        .prepare_workload_ingress(None, request.clone(), &claim)
        .expect("publication should claim the launch reservation");
    let listener = TcpListener::bind(prepared.bind_addr().expect("bind address should resolve"))
        .expect("publication listener should bind");
    let adopted = prepared
        .adopt_std(listener)
        .expect("publication listener should activate the exact lease");
    let route = RunningIngressRoute::start(
        ExpectedRoute {
            listener_id: ListenerId::for_workload_listener("tenant-a/workload-a", "bounded"),
            request: request.clone(),
            upstream: upstream_addr,
        },
        adopted,
        1,
    )
    .expect("bounded ingress route should start");

    let first = TcpStream::connect(route.bound_addr).expect("first client should connect");
    accepted_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("first connection should reach its upstream");
    wait_for_counter(&route.active_connections, 1, "active connection");
    let second = TcpStream::connect(route.bound_addr).expect("second client should connect");
    wait_for_counter(&route.rejected_connections, 1, "rejected connection");
    assert_eq!(route.peak_connections.load(Ordering::Acquire), 1);
    let active_connections = Arc::clone(&route.active_connections);

    drop(second);
    drop(first);
    drop(route);
    assert_eq!(
        active_connections.load(Ordering::Acquire),
        0,
        "listener settlement must wait for every transitively owned connection worker"
    );
    upstream_worker
        .join()
        .expect("upstream worker should finish after route shutdown");
    assert_eq!(
        port_authority
            .inspect(request.lease_id())
            .expect("settled lease should inspect")
            .expect("settled lease evidence should remain")
            .phase(),
        nimbus_network::PortLeasePhase::Released,
        "route drop must join every tracked connection before settling the lease"
    );
}

fn wait_for_counter(counter: &AtomicUsize, minimum: usize, label: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while counter.load(Ordering::Acquire) < minimum {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {label}; observed {}",
            counter.load(Ordering::Acquire)
        );
        std::thread::yield_now();
    }
}
