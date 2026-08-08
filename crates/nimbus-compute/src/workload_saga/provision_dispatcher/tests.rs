use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkExposure, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkPortAssignmentMode, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadOwnerEvidenceDigest, WorkloadProvisionDisposition,
    WorkloadProvisionEffectResult, WorkloadProvisionSubjects, WorkloadProvisionSuccessEvidence,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaStore,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;
use crate::workload_saga::{WorkloadProvisionDecision, WorkloadProvisionSymbolicAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProviderCall {
    operation: &'static str,
    step: WorkloadProvisionStep,
    mode: WorkloadProvisionCommandMode,
}

#[derive(Default)]
struct RecordingProvider {
    calls: Mutex<Vec<ProviderCall>>,
}

impl RecordingProvider {
    fn outcome(
        &self,
        operation: &'static str,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls
            .lock()
            .expect("provider call lock is healthy")
            .push(ProviderCall {
                operation,
                step: command.step(),
                mode: command.mode(),
            });
        WorkloadProvisionInspectionResult::Ambiguous {
            attempt_id: command.attempt_id().clone(),
            dispatch_epoch: command.dispatch_epoch(),
            provider_target: command.provider_target().clone(),
        }
    }

    fn calls(&self) -> Vec<ProviderCall> {
        self.calls
            .lock()
            .expect("provider call lock is healthy")
            .clone()
    }
}

macro_rules! effect_capability {
    ($trait_name:ident, $execute:literal, $inspect:literal) => {
        impl $trait_name for RecordingProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome($execute, command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome($inspect, command) })
            }
        }
    };
}

macro_rules! inspection_capability {
    ($trait_name:ident, $inspect:literal) => {
        impl $trait_name for RecordingProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome($inspect, command) })
            }
        }
    };
}

effect_capability!(
    NetworkReservationCapability,
    "reserve.execute",
    "reserve.inspect"
);
effect_capability!(
    WorkloadPreparationCapability,
    "prepare.execute",
    "prepare.inspect"
);
effect_capability!(
    NetworkAttachmentCapability,
    "attach.execute",
    "attach.inspect"
);
inspection_capability!(
    WorkloadActivationPrerequisiteCapability,
    "activation_prerequisite.inspect"
);
effect_capability!(
    WorkloadActivationCapability,
    "activate.execute",
    "activate.inspect"
);
inspection_capability!(WorkloadReadinessCapability, "readiness.inspect");
effect_capability!(
    IngressPublicationCapability,
    "publish.execute",
    "publish.inspect"
);
inspection_capability!(
    IngressPublicationInspectionCapability,
    "publication_observation.inspect"
);

impl crate::workload_projection::WorkloadExecutionObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadExecutionObservationRequest,
    ) -> crate::workload_projection::WorkloadExecutionObservationFuture<'a> {
        Box::pin(async { crate::workload_projection::WorkloadProviderObservation::Ambiguous })
    }
}

impl crate::workload_projection::WorkloadIngressObservationCapability for RecordingProvider {
    fn observe<'a>(
        &'a self,
        _request: &'a crate::workload_projection::WorkloadIngressObservationRequest,
    ) -> crate::workload_projection::WorkloadIngressObservationFuture<'a> {
        Box::pin(async { crate::workload_projection::WorkloadProviderObservation::Ambiguous })
    }
}

struct StaticSourceAuthority {
    result: Mutex<Result<WorkloadProvisionSourceEvidence, WorkloadProvisionSourceAuthorityError>>,
    calls: AtomicUsize,
    requested_keys: Mutex<Vec<WorkloadSagaKey>>,
}

impl StaticSourceAuthority {
    fn exact(record: &WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(record.active_intent().source().clone())),
            calls: AtomicUsize::new(0),
            requested_keys: Mutex::new(Vec::new()),
        })
    }

    fn with_evidence(evidence: WorkloadProvisionSourceEvidence) -> Arc<Self> {
        Arc::new(Self {
            result: Mutex::new(Ok(evidence)),
            calls: AtomicUsize::new(0),
            requested_keys: Mutex::new(Vec::new()),
        })
    }
}

impl WorkloadProvisionSourceAuthority for StaticSourceAuthority {
    fn current_source<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
        _identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.requested_keys
                .lock()
                .expect("requested source key lock is healthy")
                .push(key.clone());
            self.result
                .lock()
                .expect("source result lock is healthy")
                .clone()
        })
    }
}

#[derive(Default)]
struct RecordingStore {
    loaded: Mutex<Option<WorkloadSagaRecord>>,
    compare_and_swaps: AtomicUsize,
    loads: AtomicUsize,
}

impl RecordingStore {
    fn with_loaded(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            loaded: Mutex::new(Some(record)),
            ..Self::default()
        })
    }
}

impl WorkloadSagaStore for RecordingStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            Ok(self
                .loaded
                .lock()
                .expect("loaded record lock is healthy")
                .clone())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.compare_and_swaps.fetch_add(1, Ordering::AcqRel);
            *self.loaded.lock().expect("loaded record lock is healthy") = Some(next);
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

fn provider_reports(extra_attachment_family: bool) -> NetworkCapabilityRegistry {
    let attachment_provider = NetworkProviderId::for_registration_key("fixture-attachment");
    let ingress_provider = NetworkProviderId::for_registration_key("fixture-ingress");
    let address_families = if extra_attachment_family {
        vec![NetworkAddressFamily::Ipv4, NetworkAddressFamily::Ipv6]
    } else {
        vec![NetworkAddressFamily::Ipv4]
    };
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let bundle = NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            attachment_provider,
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            address_families,
            lifecycle.clone(),
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
        NetworkIngressProviderRegistration::new(
            ingress_provider,
            NetworkEndpointCapabilitySet::new(
                [NetworkAddressFamily::Ipv4],
                [NetworkBindRealmKind::Host],
                [NetworkExposure::Loopback],
                [PortProtocol::Tcp],
                [NetworkPortAssignmentMode::ProviderAssigned],
            ),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            lifecycle,
            NetworkSovereigntyCapabilities::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        ),
    );
    NetworkCapabilityRegistry::new([bundle]).expect("fixture provider reports should validate")
}

fn provision_capabilities(provider: Arc<RecordingProvider>) -> WorkloadProvisionCapabilityRegistry {
    WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            provider.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
            provider.clone(),
        )],
        [IngressProvisionCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            provider,
        )],
    )
    .expect("fixture provision capabilities should validate")
}

fn proposal(record: &WorkloadSagaRecord) -> ProposedWorkloadProvisionTransition {
    let WorkloadProvisionDecision::Proposed(proposal) =
        WorkloadProvisionDecision::plan(record).expect("fixture phase should be decidable")
    else {
        panic!("fixture phase should propose a transition");
    };
    proposal
}

fn dispatcher(
    record: &WorkloadSagaRecord,
    provider: Arc<RecordingProvider>,
) -> WorkloadProvisionDispatcher {
    WorkloadProvisionDispatcher::new(
        StaticSourceAuthority::exact(record),
        provider_reports(false),
        Arc::new(provision_capabilities(provider)),
    )
}

async fn route_phase(
    label: &str,
    phase: WorkloadSagaPhase,
) -> (
    ConfirmedWorkloadProvisionTransition,
    Arc<RecordingProvider>,
    Arc<RecordingStore>,
) {
    let record = provision_record(
        label,
        phase,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let proposal = proposal(&record);
    let provider = Arc::new(RecordingProvider::default());
    let dispatcher = dispatcher(&record, provider.clone());
    let store = RecordingStore::with_loaded(record.clone());
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = dispatcher
        .confirm_transition(&coordinator, &record, &proposal)
        .await
        .expect("fresh exact proposal should confirm");
    dispatcher
        .dispatch_confirmed(&confirmed)
        .await
        .expect("exact capability should route")
        .expect("effectful phase should produce a result");
    (confirmed, provider, store)
}

fn activation_transition(
    record: &WorkloadSagaRecord,
) -> (WorkloadSagaRecord, ProposedWorkloadProvisionTransition) {
    let prerequisite = proposal(record).into_candidate();
    let claim = prerequisite
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("prerequisite claim should exist");
    let WorkloadProvisionSubjects::Readiness { network, execution } = claim.attempt().subjects()
    else {
        panic!("prerequisite attempt should retain readiness subjects");
    };
    let result = WorkloadProvisionEffectResult::Succeeded {
        attempt_id: claim.attempt().attempt_id().clone(),
        evidence: WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("activation-prerequisites-ready"),
        },
    };
    let WorkloadProvisionDecision::Proposed(activation) =
        WorkloadProvisionDecision::reduce(&prerequisite, result)
            .expect("exact prerequisite success should propose activation")
    else {
        panic!("prerequisite success should propose activation");
    };
    assert_eq!(
        activation.action_after_confirmation(),
        Some(WorkloadProvisionSymbolicAction::StartExactAttempt)
    );
    (prerequisite, activation)
}

#[tokio::test]
async fn current_source_mismatch_rejects_before_attempt_cas() {
    let record = provision_record(
        "source-mismatch",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let crossed = provision_record(
        "crossed-source",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let provider = Arc::new(RecordingProvider::default());
    let source_authority =
        StaticSourceAuthority::with_evidence(crossed.active_intent().source().clone());
    let dispatcher = WorkloadProvisionDispatcher::new(
        source_authority.clone(),
        provider_reports(false),
        Arc::new(provision_capabilities(provider.clone())),
    );
    let store = RecordingStore::with_loaded(record.clone());
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    assert!(matches!(
        dispatcher
            .confirm_transition(&coordinator, &record, &proposal(&record))
            .await,
        Err(WorkloadProvisionDispatchError::CurrentSourceMismatch { .. })
    ));
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 0);
    assert!(provider.calls().is_empty());
    assert_eq!(
        source_authority
            .requested_keys
            .lock()
            .expect("requested source key lock is healthy")
            .as_slice(),
        [record.key().clone()],
        "source freshness must be scoped by the tenant-qualified saga key"
    );
}

#[tokio::test]
async fn provider_report_digest_mismatch_rejects_before_effect() {
    let record = provision_record(
        "provider-report-mismatch",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let provider = Arc::new(RecordingProvider::default());
    let exact = dispatcher(&record, provider.clone());
    let store = RecordingStore::with_loaded(record.clone());
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    let confirmed = exact
        .confirm_transition(&coordinator, &record, &proposal(&record))
        .await
        .expect("exact report should permit claim CAS");
    assert_eq!(store.compare_and_swaps.load(Ordering::Acquire), 1);

    let changed = WorkloadProvisionDispatcher::new(
        StaticSourceAuthority::exact(
            confirmed
                .confirmed_record()
                .expect("confirmation should retain durable truth"),
        ),
        provider_reports(true),
        Arc::new(provision_capabilities(provider.clone())),
    );
    assert!(matches!(
        changed.dispatch_confirmed(&confirmed).await,
        Err(WorkloadProvisionDispatchError::CurrentProviderReportMismatch { .. })
    ));
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn network_steps_bind_exact_selected_role_provider_and_digest() {
    for (label, phase, step) in [
        (
            "network-reserve-target",
            WorkloadSagaPhase::IntentCommitted,
            WorkloadProvisionStep::ReserveNetwork,
        ),
        (
            "network-attach-target",
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadProvisionStep::AttachNetwork,
        ),
    ] {
        let (confirmed, _, _) = route_phase(label, phase).await;
        let command = confirmed.command().expect("network command should exist");
        assert_eq!(command.step(), step);
        assert!(matches!(
            command.provider_target(),
            WorkloadProvisionProviderTarget::Network {
                role: NetworkCapabilityRole::Attachment,
                provider_id,
                ..
            } if provider_id == &NetworkProviderId::for_registration_key("fixture-attachment")
        ));
    }
}

#[tokio::test]
async fn prepare_and_activate_bind_execution_provider_without_network_role() {
    let (prepare, _, _) = route_phase(
        "execution-prepare-target",
        WorkloadSagaPhase::NetworkReserved,
    )
    .await;
    assert!(matches!(
        prepare
            .command()
            .expect("prepare command should exist")
            .provider_target(),
        WorkloadProvisionProviderTarget::Execution { provider_id, .. }
            if provider_id == &WorkloadExecutionProviderId::for_registration_key("fixture-execution")
    ));

    let record = provision_record(
        "execution-activate-target",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let (pending, activation) = activation_transition(&record);
    let provider = Arc::new(RecordingProvider::default());
    let dispatcher = dispatcher(&pending, provider);
    let store = RecordingStore::with_loaded(pending.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = dispatcher
        .confirm_transition(&coordinator, &pending, &activation)
        .await
        .expect("activation should confirm");
    assert!(matches!(
        confirmed
            .command()
            .expect("activation command should exist")
            .provider_target(),
        WorkloadProvisionProviderTarget::Execution { provider_id, .. }
            if provider_id == &WorkloadExecutionProviderId::for_registration_key("fixture-execution")
    ));
}

#[tokio::test]
async fn resource_free_network_steps_fabricate_no_provider_target() {
    let record = provision_record(
        "resource-free-dispatch",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let provider = Arc::new(RecordingProvider::default());
    let source_authority = StaticSourceAuthority::exact(&record);
    let dispatcher = WorkloadProvisionDispatcher::new(
        source_authority,
        NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"),
        Arc::new(provision_capabilities(provider.clone())),
    );
    let store = RecordingStore::with_loaded(record.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = dispatcher
        .confirm_transition(&coordinator, &record, &proposal(&record))
        .await
        .expect("resource-free transition should confirm");
    assert!(confirmed.command().is_none());
    assert_eq!(
        dispatcher
            .dispatch_confirmed(&confirmed)
            .await
            .expect("resource-free dispatch should succeed"),
        None
    );
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn reserve_command_mapping_is_exact() {
    let (_, provider, _) = route_phase("reserve-mapping", WorkloadSagaPhase::IntentCommitted).await;
    assert_eq!(
        provider.calls(),
        [ProviderCall {
            operation: "reserve.execute",
            step: WorkloadProvisionStep::ReserveNetwork,
            mode: WorkloadProvisionCommandMode::Execute,
        }]
    );
}

#[tokio::test]
async fn prepare_command_mapping_is_exact() {
    let (_, provider, _) = route_phase("prepare-mapping", WorkloadSagaPhase::NetworkReserved).await;
    assert_eq!(provider.calls()[0].operation, "prepare.execute");
}

#[tokio::test]
async fn attach_command_mapping_is_exact() {
    let (_, provider, _) = route_phase("attach-mapping", WorkloadSagaPhase::WorkloadPrepared).await;
    assert_eq!(provider.calls()[0].operation, "attach.execute");
}

#[tokio::test]
async fn activation_prerequisite_command_mapping_is_exact() {
    let (_, provider, _) = route_phase(
        "activation-prerequisite-mapping",
        WorkloadSagaPhase::NetworkAttached,
    )
    .await;
    assert_eq!(
        provider.calls(),
        [ProviderCall {
            operation: "activation_prerequisite.inspect",
            step: WorkloadProvisionStep::InspectActivationPrerequisites,
            mode: WorkloadProvisionCommandMode::Inspect,
        }]
    );
}

#[tokio::test]
async fn activate_command_mapping_is_exact() {
    let record = provision_record(
        "activate-mapping",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let (pending, activation) = activation_transition(&record);
    let provider = Arc::new(RecordingProvider::default());
    let dispatcher = dispatcher(&pending, provider.clone());
    let store = RecordingStore::with_loaded(pending.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = dispatcher
        .confirm_transition(&coordinator, &pending, &activation)
        .await
        .expect("activation should confirm");
    dispatcher
        .dispatch_confirmed(&confirmed)
        .await
        .expect("activation should route");
    assert_eq!(provider.calls()[0].operation, "activate.execute");
}

#[tokio::test]
async fn workload_readiness_command_mapping_is_exact() {
    let (_, provider, _) =
        route_phase("readiness-mapping", WorkloadSagaPhase::WorkloadActivated).await;
    assert_eq!(
        provider.calls(),
        [ProviderCall {
            operation: "readiness.inspect",
            step: WorkloadProvisionStep::InspectWorkloadReadiness,
            mode: WorkloadProvisionCommandMode::Inspect,
        }]
    );
}

#[tokio::test]
async fn publication_command_mapping_is_exact() {
    let (_, provider, _) = route_phase("publish-mapping", WorkloadSagaPhase::Ready).await;
    assert_eq!(provider.calls()[0].operation, "publish.execute");
}

#[tokio::test]
async fn publication_observation_command_mapping_is_exact() {
    let (_, provider, _) =
        route_phase("publication-observation", WorkloadSagaPhase::Published).await;
    assert_eq!(
        provider.calls(),
        [ProviderCall {
            operation: "publication_observation.inspect",
            step: WorkloadProvisionStep::ObservePublication,
            mode: WorkloadProvisionCommandMode::Inspect,
        }]
    );
}

#[tokio::test]
async fn prepare_attach_and_activate_cannot_publish() {
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        let (_, provider, _) = route_phase(&format!("not-published-{phase:?}"), phase).await;
        assert!(
            provider
                .calls()
                .iter()
                .all(|call| !call.operation.starts_with("publish"))
        );
    }
}

#[tokio::test]
async fn withheld_and_prepare_only_emit_no_provider_command() {
    let withheld = provision_record(
        "withheld-no-command",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let provider = Arc::new(RecordingProvider::default());
    let dispatcher = WorkloadProvisionDispatcher::new(
        StaticSourceAuthority::exact(&withheld),
        NetworkCapabilityRegistry::new([]).expect("empty report registry should validate"),
        Arc::new(provision_capabilities(provider.clone())),
    );
    let store = RecordingStore::with_loaded(withheld.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = dispatcher
        .confirm_transition(&coordinator, &withheld, &proposal(&withheld))
        .await
        .expect("withheld observation should confirm");
    assert!(confirmed.command().is_none());

    let prepare_only = provision_record(
        "prepare-only-no-command",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    assert_eq!(
        WorkloadProvisionDecision::plan(&prepare_only).expect("prepare-only should be decidable"),
        WorkloadProvisionDecision::Wait
    );
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn exact_registry_never_falls_back_to_alternative_provider() {
    let record = provision_record(
        "no-fallback",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let exact_provider = Arc::new(RecordingProvider::default());
    let exact_dispatcher = dispatcher(&record, exact_provider);
    let store = RecordingStore::with_loaded(record.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = exact_dispatcher
        .confirm_transition(&coordinator, &record, &proposal(&record))
        .await
        .expect("exact claim should confirm");

    let alternative = Arc::new(RecordingProvider::default());
    let alternative_capabilities = WorkloadProvisionCapabilityRegistry::new(
        [NetworkAttachmentProvisionCapabilities::new(
            NetworkProviderId::for_registration_key("alternative-attachment"),
            alternative.clone(),
        )],
        [WorkloadExecutionProvisionCapabilities::new(
            WorkloadExecutionProviderId::for_registration_key("alternative-execution"),
            alternative.clone(),
        )],
        [IngressProvisionCapabilities::new(
            NetworkProviderId::for_registration_key("alternative-ingress"),
            alternative.clone(),
        )],
    )
    .expect("alternative registry should validate");
    let dispatcher = WorkloadProvisionDispatcher::new(
        StaticSourceAuthority::exact(
            confirmed
                .confirmed_record()
                .expect("confirmation should retain durable truth"),
        ),
        provider_reports(false),
        Arc::new(alternative_capabilities),
    );
    assert!(matches!(
        dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadProvisionDispatchError::MissingCapability { .. })
    ));
    assert!(alternative.calls().is_empty());
}

#[tokio::test]
async fn fresh_recovery_reopens_store_and_inspects_exact_provider() {
    let record = provision_record(
        "dispatcher-recovery",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let pending = proposal(&record).into_candidate();
    let provider = Arc::new(RecordingProvider::default());
    let dispatcher = dispatcher(&pending, provider.clone());
    let store = RecordingStore::with_loaded(pending.clone());
    let coordinator = WorkloadSagaCoordinator::new(store.clone());
    dispatcher
        .inspect_recovery(&coordinator, pending.key())
        .await
        .expect("fresh recovery should route inspection");
    assert_eq!(store.loads.load(Ordering::Acquire), 1);
    assert_eq!(provider.calls()[0].operation, "prepare.inspect");
}

#[test]
fn capability_registry_rejects_duplicate_and_cross_role_authority() {
    let provider = Arc::new(RecordingProvider::default());
    assert!(matches!(
        WorkloadProvisionCapabilityRegistry::new(
            [
                NetworkAttachmentProvisionCapabilities::new(
                    NetworkProviderId::for_registration_key("duplicate"),
                    provider.clone(),
                ),
                NetworkAttachmentProvisionCapabilities::new(
                    NetworkProviderId::for_registration_key("duplicate"),
                    provider.clone(),
                ),
            ],
            [],
            []
        ),
        Err(WorkloadProvisionCapabilityRegistryError::DuplicateAttachment { .. })
    ));
    assert!(matches!(
        WorkloadProvisionCapabilityRegistry::new(
            [NetworkAttachmentProvisionCapabilities::new(
                NetworkProviderId::for_registration_key("cross-role"),
                provider.clone(),
            )],
            [],
            [IngressProvisionCapabilities::new(
                NetworkProviderId::for_registration_key("cross-role"),
                provider,
            )]
        ),
        Err(WorkloadProvisionCapabilityRegistryError::NetworkRoleConflict { .. })
    ));
}
