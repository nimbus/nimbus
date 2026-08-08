use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkExposure,
    NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkSovereigntyCapabilities, PortProtocol,
};
use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadExecutionProviderId, WorkloadFailureEvidence,
    WorkloadOwnerEvidenceDigest, WorkloadProvisionDisposition, WorkloadProvisionInspectionResult,
    WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;
use crate::workload_saga::{
    ConfirmedWorkloadProvisionCommand, IngressProvisionCapabilities, IngressPublicationCapability,
    IngressPublicationInspectionCapability, NetworkAttachmentCapability,
    NetworkAttachmentProvisionCapabilities, NetworkReservationCapability,
    WorkloadActivationCapability, WorkloadActivationPrerequisiteCapability,
    WorkloadExecutionProvisionCapabilities, WorkloadPreparationCapability,
    WorkloadProvisionCapabilityFuture, WorkloadProvisionCapabilityRegistry,
    WorkloadProvisionCommandMode, WorkloadProvisionDispatcher, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadReadinessCapability,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProviderCall {
    step: WorkloadProvisionStep,
    mode: WorkloadProvisionCommandMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderBehavior {
    Succeed,
    DefiniteFailureAt(WorkloadProvisionStep),
    InProgressAt(WorkloadProvisionStep),
    AmbiguousAt(WorkloadProvisionStep),
    AbsentWhenInspectedAt(WorkloadProvisionStep),
    AmbiguousExecuteAbsentInspectAt(WorkloadProvisionStep),
}

struct RecordingProvider {
    behavior: ProviderBehavior,
    source_change: Mutex<
        Option<(
            WorkloadProvisionStep,
            Arc<StaticSourceAuthority>,
            WorkloadProvisionSourceEvidence,
        )>,
    >,
    calls: Mutex<Vec<ProviderCall>>,
}

impl RecordingProvider {
    fn new(behavior: ProviderBehavior) -> Arc<Self> {
        Arc::new(Self {
            behavior,
            source_change: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn changing_source_after(
        step: WorkloadProvisionStep,
        source: Arc<StaticSourceAuthority>,
        replacement: WorkloadProvisionSourceEvidence,
    ) -> Arc<Self> {
        Arc::new(Self {
            behavior: ProviderBehavior::Succeed,
            source_change: Mutex::new(Some((step, source, replacement))),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn outcome(
        &self,
        command: &ConfirmedWorkloadProvisionCommand,
    ) -> WorkloadProvisionInspectionResult {
        self.calls
            .lock()
            .expect("provider call lock is healthy")
            .push(ProviderCall {
                step: command.step(),
                mode: command.mode(),
            });
        let source_change = {
            let mut change = self
                .source_change
                .lock()
                .expect("source-change lock is healthy");
            if change
                .as_ref()
                .is_some_and(|(step, _, _)| *step == command.step())
            {
                change.take()
            } else {
                None
            }
        };
        if let Some((_, source, replacement)) = source_change {
            source.replace(replacement);
        }
        match self.behavior {
            ProviderBehavior::DefiniteFailureAt(step) if step == command.step() => {
                WorkloadProvisionInspectionResult::DefiniteFailure {
                    attempt_id: command.attempt_id().clone(),
                    dispatch_epoch: command.dispatch_epoch(),
                    provider_target: command.provider_target().clone(),
                    failure: WorkloadFailureEvidence::new(
                        "fixture_failure",
                        WorkloadOwnerEvidenceDigest::sha256("fixture failure"),
                    )
                    .expect("fixture failure is valid"),
                }
            }
            ProviderBehavior::InProgressAt(step) if step == command.step() => {
                WorkloadProvisionInspectionResult::InProgress {
                    attempt_id: command.attempt_id().clone(),
                    dispatch_epoch: command.dispatch_epoch(),
                    provider_target: command.provider_target().clone(),
                    evidence: WorkloadOwnerEvidenceDigest::sha256("fixture in progress"),
                }
            }
            ProviderBehavior::AmbiguousAt(step) if step == command.step() => {
                WorkloadProvisionInspectionResult::Ambiguous {
                    attempt_id: command.attempt_id().clone(),
                    dispatch_epoch: command.dispatch_epoch(),
                    provider_target: command.provider_target().clone(),
                }
            }
            ProviderBehavior::AbsentWhenInspectedAt(step)
                if step == command.step()
                    && command.mode() == WorkloadProvisionCommandMode::Inspect =>
            {
                WorkloadProvisionInspectionResult::Absent {
                    evidence: command
                        .absence_evidence(WorkloadOwnerEvidenceDigest::sha256("fixture absent")),
                }
            }
            ProviderBehavior::AmbiguousExecuteAbsentInspectAt(step)
                if step == command.step()
                    && command.mode() == WorkloadProvisionCommandMode::Inspect =>
            {
                WorkloadProvisionInspectionResult::Absent {
                    evidence: command.absence_evidence(WorkloadOwnerEvidenceDigest::sha256(
                        "fixture cycle absent",
                    )),
                }
            }
            ProviderBehavior::AmbiguousExecuteAbsentInspectAt(step) if step == command.step() => {
                WorkloadProvisionInspectionResult::Ambiguous {
                    attempt_id: command.attempt_id().clone(),
                    dispatch_epoch: command.dispatch_epoch(),
                    provider_target: command.provider_target().clone(),
                }
            }
            _ => WorkloadProvisionInspectionResult::Succeeded {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence: crate::workload_saga::test_support::success_for(
                    command.claim().attempt(),
                ),
            },
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
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

macro_rules! inspection_capability {
    ($trait_name:ident) => {
        impl $trait_name for RecordingProvider {
            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadProvisionCommand,
            ) -> WorkloadProvisionCapabilityFuture<'a> {
                Box::pin(async move { self.outcome(command) })
            }
        }
    };
}

effect_capability!(NetworkReservationCapability);
effect_capability!(WorkloadPreparationCapability);
effect_capability!(NetworkAttachmentCapability);
inspection_capability!(WorkloadActivationPrerequisiteCapability);
effect_capability!(WorkloadActivationCapability);
inspection_capability!(WorkloadReadinessCapability);
effect_capability!(IngressPublicationCapability);
inspection_capability!(IngressPublicationInspectionCapability);

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

struct StaticSourceAuthority(Mutex<WorkloadProvisionSourceEvidence>);

impl StaticSourceAuthority {
    fn exact(record: &WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self(Mutex::new(record.active_intent().source().clone())))
    }

    fn replace(&self, evidence: WorkloadProvisionSourceEvidence) {
        *self.0.lock().expect("source authority lock is healthy") = evidence;
    }
}

impl WorkloadProvisionSourceAuthority for StaticSourceAuthority {
    fn current_source<'a>(
        &'a self,
        _key: &'a nimbus_workloads::WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            let evidence = self.0.lock().expect("source authority lock is healthy");
            if evidence.source_identity() != identity {
                return Err(WorkloadProvisionSourceAuthorityError::NotFound);
            }
            Ok(evidence.clone())
        })
    }
}

#[derive(Default)]
struct DurableTestStore {
    record: Mutex<Option<WorkloadSagaRecord>>,
    next_cas_fault: Mutex<Option<CasFault>>,
    loads: AtomicUsize,
    compare_and_swaps: AtomicUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CasFault {
    AmbiguousBeforeApply,
    AmbiguousAfterApply,
}

impl DurableTestStore {
    fn with_record(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(Some(record)),
            ..Self::default()
        })
    }

    fn with_record_and_fault(record: WorkloadSagaRecord, fault: CasFault) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(Some(record)),
            next_cas_fault: Mutex::new(Some(fault)),
            ..Self::default()
        })
    }

    fn counts(&self) -> (usize, usize) {
        (
            self.loads.load(Ordering::Acquire),
            self.compare_and_swaps.load(Ordering::Acquire),
        )
    }

    fn record(&self) -> WorkloadSagaRecord {
        self.record
            .lock()
            .expect("durable store lock is healthy")
            .clone()
            .expect("fixture store should retain a record")
    }
}

impl WorkloadSagaStore for DurableTestStore {
    fn load<'a>(
        &'a self,
        key: &'a nimbus_workloads::WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.loads.fetch_add(1, Ordering::AcqRel);
            let record = self.record.lock().expect("durable store lock is healthy");
            if record.as_ref().is_some_and(|current| current.key() != key) {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            Ok(record.clone())
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.compare_and_swaps.fetch_add(1, Ordering::AcqRel);
            let mut current = self.record.lock().expect("durable store lock is healthy");
            if current.as_ref() == Some(&next) {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            let matches = match (expected, current.as_ref()) {
                (WorkloadSagaExpected::Missing, None) => true,
                (WorkloadSagaExpected::Revision(expected), Some(record)) => {
                    expected == record.revision()
                }
                _ => false,
            };
            if !matches {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: current.as_ref().map(WorkloadSagaRecord::revision),
                });
            }
            match self
                .next_cas_fault
                .lock()
                .expect("CAS fault lock is healthy")
                .take()
            {
                Some(CasFault::AmbiguousBeforeApply) => {
                    return Err(WorkloadSagaStoreError::Ambiguous);
                }
                Some(CasFault::AmbiguousAfterApply) => {
                    *current = Some(next);
                    return Err(WorkloadSagaStoreError::Ambiguous);
                }
                None => {}
            }
            *current = Some(next);
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

fn provider_reports() -> NetworkCapabilityRegistry {
    let attachment_provider = NetworkProviderId::for_registration_key("fixture-attachment");
    let ingress_provider = NetworkProviderId::for_registration_key("fixture-ingress");
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    NetworkCapabilityRegistry::new([NetworkCapabilityBundle::new(
        NetworkAttachmentProviderRegistration::new(
            attachment_provider,
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
            [NetworkAddressFamily::Ipv4],
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
    )])
    .expect("fixture provider reports validate")
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
    .expect("fixture provision registry validates")
}

fn driver(
    store: Arc<DurableTestStore>,
    source_record: &WorkloadSagaRecord,
    provider: Arc<RecordingProvider>,
) -> WorkloadProvisionDriver {
    driver_with_source(store, StaticSourceAuthority::exact(source_record), provider)
}

fn driver_with_source(
    store: Arc<DurableTestStore>,
    source: Arc<StaticSourceAuthority>,
    provider: Arc<RecordingProvider>,
) -> WorkloadProvisionDriver {
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let dispatcher = Arc::new(WorkloadProvisionDispatcher::new(
        source,
        provider_reports(),
        Arc::new(provision_capabilities(provider)),
    ));
    WorkloadProvisionDriver::new(coordinator, dispatcher)
}

fn changed_source(record: &WorkloadSagaRecord) -> WorkloadProvisionSourceEvidence {
    let source = record.active_intent().source();
    WorkloadProvisionSourceEvidence::standalone_sandbox(
        source.source_identity().clone(),
        WorkloadProvisionSourceGeneration::new(99),
        WorkloadProvisionSourceResourceVersion::new("changed-after-effect")
            .expect("changed fixture source version is valid"),
        record.active_intent().executable().content_digest(),
        source.attachment_provider_id().clone(),
        source.execution_provider_id().clone(),
    )
    .expect("changed fixture source evidence is valid")
}

const ORDERED_STEPS: [WorkloadProvisionStep; 8] = [
    WorkloadProvisionStep::ReserveNetwork,
    WorkloadProvisionStep::PrepareWorkload,
    WorkloadProvisionStep::AttachNetwork,
    WorkloadProvisionStep::InspectActivationPrerequisites,
    WorkloadProvisionStep::ActivateWorkload,
    WorkloadProvisionStep::InspectWorkloadReadiness,
    WorkloadProvisionStep::Publish,
    WorkloadProvisionStep::ObservePublication,
];

#[tokio::test]
async fn complete_run_confirms_every_transition_and_publishes_only_after_readiness() {
    let initial = provision_record(
        "complete-driver",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = Arc::new(DurableTestStore::default());
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let driver = driver(store.clone(), &initial, provider.clone());

    let run = driver
        .submit_and_drive(initial.key().clone(), initial.active_intent().clone())
        .await
        .expect("complete exact provision should succeed");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Observed);
    assert_eq!(run.record().phase(), WorkloadSagaPhase::Observed);
    let calls = provider.calls();
    assert_eq!(
        calls.iter().map(|call| call.step).collect::<Vec<_>>(),
        ORDERED_STEPS
    );
    assert_eq!(
        calls.iter().map(|call| call.mode).collect::<Vec<_>>(),
        [
            WorkloadProvisionCommandMode::Execute,
            WorkloadProvisionCommandMode::Execute,
            WorkloadProvisionCommandMode::Execute,
            WorkloadProvisionCommandMode::Inspect,
            WorkloadProvisionCommandMode::Execute,
            WorkloadProvisionCommandMode::Inspect,
            WorkloadProvisionCommandMode::Execute,
            WorkloadProvisionCommandMode::Inspect,
        ]
    );
    assert_eq!(store.counts(), (1, 16));
}

#[tokio::test]
async fn definite_failure_never_dispatches_a_later_step() {
    let source_phases = [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
    ];
    for (index, failed_step) in ORDERED_STEPS.into_iter().enumerate() {
        let initial = provision_record(
            &format!("failure-{index}"),
            WorkloadSagaPhase::IntentCommitted,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        );
        let store = Arc::new(DurableTestStore::default());
        let provider = RecordingProvider::new(ProviderBehavior::DefiniteFailureAt(failed_step));
        let driver = driver(store, &initial, provider.clone());

        let run = driver
            .submit_and_drive(initial.key().clone(), initial.active_intent().clone())
            .await
            .expect("definite failure should return durable halted truth");

        assert_eq!(
            run.disposition(),
            WorkloadProvisionRunDisposition::DefiniteFailure
        );
        assert_eq!(run.record().phase(), source_phases[index]);
        let Some(WorkloadProvisionDisposition::DefiniteFailure { claim, failure }) =
            run.record().provision_disposition()
        else {
            panic!("definite failure claim and evidence must be retained");
        };
        assert_eq!(failure.code(), "fixture_failure");
        assert_eq!(claim.attempt().step(), failed_step);
        assert_eq!(claim.dispatch_epoch().as_u64(), 0);
        assert_eq!(provider.calls().len(), index + 1);
        assert_eq!(
            provider.calls().last().map(|call| call.step),
            Some(failed_step)
        );
    }
}

#[tokio::test]
async fn inspection_in_progress_never_retries() {
    let initial = provision_record(
        "bounded-wait",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTestStore::with_record(initial.clone());
    let provider = RecordingProvider::new(ProviderBehavior::InProgressAt(
        WorkloadProvisionStep::ReserveNetwork,
    ));
    let driver = driver(store.clone(), &initial, provider.clone());

    let first = driver
        .resume(initial.key())
        .await
        .expect("in-progress provider should return bounded durable truth");
    assert_eq!(
        first.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert!(matches!(
        first.record().provision_disposition(),
        Some(WorkloadProvisionDisposition::InspectionRequired(_))
    ));
    assert_eq!(
        provider.calls(),
        [
            ProviderCall {
                step: WorkloadProvisionStep::ReserveNetwork,
                mode: WorkloadProvisionCommandMode::Execute,
            },
            ProviderCall {
                step: WorkloadProvisionStep::ReserveNetwork,
                mode: WorkloadProvisionCommandMode::Inspect,
            },
        ]
    );

    let resumed = driver
        .resume(initial.key())
        .await
        .expect("a later trigger should inspect once and return");
    assert_eq!(
        resumed.disposition(),
        WorkloadProvisionRunDisposition::Waiting
    );
    assert_eq!(provider.calls().len(), 3);
    assert_eq!(
        provider.calls()[2].mode,
        WorkloadProvisionCommandMode::Inspect
    );
}

#[tokio::test]
async fn ambiguous_publication_inspects_before_retry() {
    let initial = provision_record(
        "ambiguous-publication",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTestStore::with_record(initial.clone());
    let provider = RecordingProvider::new(ProviderBehavior::AmbiguousAt(
        WorkloadProvisionStep::Publish,
    ));
    let driver = driver(store, &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("ambiguous publication should return bounded durable truth");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Waiting);
    assert_eq!(run.record().phase(), WorkloadSagaPhase::Ready);
    assert_eq!(
        provider
            .calls()
            .iter()
            .filter(|call| call.step == WorkloadProvisionStep::Publish)
            .copied()
            .collect::<Vec<_>>(),
        [
            ProviderCall {
                step: WorkloadProvisionStep::Publish,
                mode: WorkloadProvisionCommandMode::Execute,
            },
            ProviderCall {
                step: WorkloadProvisionStep::Publish,
                mode: WorkloadProvisionCommandMode::Inspect,
            },
        ]
    );
}

#[tokio::test]
async fn ambiguous_claim_confirmation_inspects_and_never_grants_execute_authority() {
    let initial = provision_record(
        "ambiguous-claim-driver",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store =
        DurableTestStore::with_record_and_fault(initial.clone(), CasFault::AmbiguousAfterApply);
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let driver = driver(store, &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("observed ambiguous claim should recover by inspection");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Observed);
    assert_eq!(
        provider.calls()[0].step,
        WorkloadProvisionStep::ReserveNetwork
    );
    assert_eq!(
        provider.calls()[0].mode,
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(
        provider
            .calls()
            .iter()
            .filter(|call| {
                call.step == WorkloadProvisionStep::ReserveNetwork
                    && call.mode == WorkloadProvisionCommandMode::Execute
            })
            .count(),
        0
    );
}

#[tokio::test]
async fn unresolved_claim_confirmation_returns_waiting_before_provider_effect() {
    let initial = provision_record(
        "unresolved-claim-driver",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store =
        DurableTestStore::with_record_and_fault(initial.clone(), CasFault::AmbiguousBeforeApply);
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let driver = driver(store, &initial, provider.clone());

    let run = driver
        .resume(initial.key())
        .await
        .expect("unresolved CAS must return bounded durable truth");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Waiting);
    assert_eq!(run.record(), &initial);
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn crash_after_dispatch_cas_before_effect_inspects() {
    let initial = provision_record(
        "crash-before-effect",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let pending = crate::workload_saga::test_support::first_proposed_candidate(&initial);
    let store = DurableTestStore::with_record(pending);
    let provider = RecordingProvider::new(ProviderBehavior::AbsentWhenInspectedAt(
        WorkloadProvisionStep::ReserveNetwork,
    ));
    let fresh_driver = driver(store, &initial, provider.clone());

    let run = fresh_driver
        .resume(initial.key())
        .await
        .expect("fresh recovery should inspect absence before retry");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Observed);
    assert_eq!(
        provider
            .calls()
            .iter()
            .filter(|call| call.step == WorkloadProvisionStep::ReserveNetwork)
            .copied()
            .collect::<Vec<_>>(),
        [
            ProviderCall {
                step: WorkloadProvisionStep::ReserveNetwork,
                mode: WorkloadProvisionCommandMode::Inspect,
            },
            ProviderCall {
                step: WorkloadProvisionStep::ReserveNetwork,
                mode: WorkloadProvisionCommandMode::Execute,
            },
        ]
    );
}

#[tokio::test]
async fn crash_after_effect_before_result_cas_inspects() {
    let initial = provision_record(
        "crash-after-effect",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let pending = crate::workload_saga::test_support::first_proposed_candidate(&initial);
    let store = DurableTestStore::with_record(pending);
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let fresh_driver = driver(store, &initial, provider.clone());

    let run = fresh_driver
        .resume(initial.key())
        .await
        .expect("fresh recovery should adopt the exact completed effect");

    assert_eq!(run.disposition(), WorkloadProvisionRunDisposition::Observed);
    assert_eq!(
        provider.calls()[0].step,
        WorkloadProvisionStep::ReserveNetwork
    );
    assert_eq!(
        provider.calls()[0].mode,
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(
        provider
            .calls()
            .iter()
            .filter(|call| {
                call.step == WorkloadProvisionStep::ReserveNetwork
                    && call.mode == WorkloadProvisionCommandMode::Execute
            })
            .count(),
        0
    );
}

#[tokio::test]
async fn progress_limit_never_strands_provider_result_before_successor_cas() {
    let initial = provision_record(
        "bounded-cyclic-provider",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTestStore::with_record(initial.clone());
    let provider = RecordingProvider::new(ProviderBehavior::AmbiguousExecuteAbsentInspectAt(
        WorkloadProvisionStep::ReserveNetwork,
    ));
    let driver = driver(store.clone(), &initial, provider.clone());

    assert!(matches!(
        driver.resume(initial.key()).await,
        Err(WorkloadProvisionRunError::ProgressLimit)
    ));
    assert_eq!(provider.calls().len(), MAX_DECISIONS_PER_RUN - 1);
    assert_eq!(store.counts().1, MAX_DECISIONS_PER_RUN);
    assert_eq!(
        provider.calls().last().map(|call| call.mode),
        Some(WorkloadProvisionCommandMode::Execute)
    );
    assert!(matches!(
        store.record().provision_disposition(),
        Some(WorkloadProvisionDisposition::InspectionRequired(_))
    ));
}

#[tokio::test]
async fn source_change_after_effect_cannot_block_exact_result_cas() {
    let initial = provision_record(
        "source-change-after-effect",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTestStore::with_record(initial.clone());
    let source = StaticSourceAuthority::exact(&initial);
    let provider = RecordingProvider::changing_source_after(
        WorkloadProvisionStep::ReserveNetwork,
        source.clone(),
        changed_source(&initial),
    );
    let driver = driver_with_source(store.clone(), source, provider.clone());

    assert!(matches!(
        driver.resume(initial.key()).await,
        Err(WorkloadProvisionRunError::Dispatch(
            WorkloadProvisionDispatchError::CurrentSourceMismatch { .. }
        ))
    ));
    assert_eq!(provider.calls().len(), 1);
    assert_eq!(store.record().phase(), WorkloadSagaPhase::NetworkReserved);
    assert!(matches!(
        store.record().provision_disposition(),
        Some(WorkloadProvisionDisposition::Ready)
    ));
}

#[tokio::test]
async fn stale_source_cannot_strand_inspection_of_an_authorized_effect() {
    let initial = provision_record(
        "stale-source-inspection",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let pending = crate::workload_saga::test_support::first_proposed_candidate(&initial);
    let store = DurableTestStore::with_record(pending);
    let source = StaticSourceAuthority::exact(&initial);
    source.replace(changed_source(&initial));
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let fresh_driver = driver_with_source(store.clone(), source, provider.clone());

    assert!(matches!(
        fresh_driver.resume(initial.key()).await,
        Err(WorkloadProvisionRunError::Dispatch(
            WorkloadProvisionDispatchError::CurrentSourceMismatch { .. }
        ))
    ));
    assert_eq!(provider.calls().len(), 1);
    assert_eq!(
        provider.calls()[0].mode,
        WorkloadProvisionCommandMode::Inspect
    );
    assert_eq!(store.record().phase(), WorkloadSagaPhase::NetworkReserved);
}

#[tokio::test]
async fn concurrent_dispatchers_create_one_provider_effect() {
    let initial = provision_record(
        "concurrent-driver",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTestStore::with_record(initial.clone());
    let provider = RecordingProvider::new(ProviderBehavior::Succeed);
    let driver = Arc::new(driver(store, &initial, provider.clone()));

    let (left, right) = tokio::join!(driver.resume(initial.key()), driver.resume(initial.key()),);
    let left = left.expect("left concurrent run should converge");
    let right = right.expect("right concurrent run should converge");

    assert_eq!(left.record().phase(), WorkloadSagaPhase::Observed);
    assert_eq!(right.record().phase(), WorkloadSagaPhase::Observed);
    for step in ORDERED_STEPS {
        assert_eq!(
            provider
                .calls()
                .iter()
                .filter(
                    |call| call.step == step && call.mode == WorkloadProvisionCommandMode::Execute
                )
                .count(),
            usize::from(!matches!(
                step,
                WorkloadProvisionStep::InspectActivationPrerequisites
                    | WorkloadProvisionStep::InspectWorkloadReadiness
                    | WorkloadProvisionStep::ObservePublication
            )),
            "only one exact execute authority may exist for {step:?}"
        );
    }
}
