use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkBindRealmKind, NetworkCapabilityBundle, NetworkCapabilityRegistry,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkLifecycleRequirements, NetworkManagementMode, NetworkPortAssignmentMode,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, PortProtocol,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadState, WorkloadActivationIntent,
    WorkloadExecutionAttemptId, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceIdentity, WorkloadPublicationIntent, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadRestartDispatchEpoch, WorkloadRestartEffectResult,
    WorkloadRestartEvidenceDigest, WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy,
    WorkloadRestartStep, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture,
    WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaStore, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::recovery::tests::provision_record;
use crate::workload_saga::restart_dispatcher::WorkloadRestartDispatcher;
use crate::workload_saga::restart_provider::{
    NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadExecutionQuiescenceCapability, WorkloadRestartActivationCapability,
    WorkloadRestartActivationPrerequisiteCapability, WorkloadRestartCapabilities,
    WorkloadRestartCapabilityFuture, WorkloadRestartCapabilityRegistry,
    WorkloadRestartPreparationCapability, WorkloadRestartProviderObservation,
    WorkloadRestartProviderObservationInput, WorkloadRestartReadinessCapability,
};
use crate::workload_saga::restart_resolution::WorkloadRestartResolutionFuture;
use crate::workload_saga::restart_resolution::{
    NoopWorkloadRestartResolutionFence, WorkloadRestartResolutionFence,
};
use crate::workload_saga::{
    ConfirmedWorkloadRestartCommand, WorkloadProvisionSourceAuthority,
    WorkloadProvisionSourceAuthorityError, WorkloadProvisionSourceFuture,
    WorkloadRestartAdmissionDecision, WorkloadRestartAdmissionRequest,
    WorkloadRestartCommandOutcome, WorkloadRestartDecision, WorkloadRestartSymbolicAction,
    decide_restart_admission, decide_restart_progress, test_support,
};

struct DurableStore {
    record: Mutex<WorkloadSagaRecord>,
}

impl DurableStore {
    fn new(record: WorkloadSagaRecord) -> Arc<Self> {
        Arc::new(Self {
            record: Mutex::new(record),
        })
    }

    fn record(&self) -> WorkloadSagaRecord {
        self.record
            .lock()
            .expect("durable store lock should be healthy")
            .clone()
    }
}

impl WorkloadSagaStore for DurableStore {
    fn load<'a>(
        &'a self,
        key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            let record = self
                .record
                .lock()
                .expect("durable store lock should be healthy");
            if record.key() != key {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            Ok(Some(record.clone()))
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        expected: WorkloadSagaExpected,
        next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            let mut current = self
                .record
                .lock()
                .expect("durable store lock should be healthy");
            if *current == next {
                return Ok(WorkloadSagaCommit::Unchanged);
            }
            if !matches!(
                expected,
                WorkloadSagaExpected::Revision(revision) if revision == current.revision()
            ) {
                return Err(WorkloadSagaStoreError::Conflict {
                    expected,
                    observed: Some(current.revision()),
                });
            }
            *current = next;
            Ok(WorkloadSagaCommit::Applied)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move { WorkloadSagaPage::new(&request, Vec::new(), false) })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: WorkloadRestartCandidatePageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadRestartCandidatePage> {
        Box::pin(async move { WorkloadRestartCandidatePage::new(&request, Vec::new(), false) })
    }

    fn list_for_tenant<'a>(
        &'a self,
        tenant_id: &'a TenantId,
        request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move { WorkloadSagaTenantPage::new(tenant_id, &request, Vec::new(), false) })
    }
}

struct ExactSourceAuthority {
    source: WorkloadProvisionSourceEvidence,
}

impl WorkloadProvisionSourceAuthority for ExactSourceAuthority {
    fn current_source<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
        identity: &'a WorkloadProvisionSourceIdentity,
    ) -> WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            if self.source.source_identity() != identity {
                return Err(WorkloadProvisionSourceAuthorityError::NotFound);
            }
            Ok(self.source.clone())
        })
    }
}

#[derive(Debug, Clone, Copy)]
enum ScriptedOutcome {
    Succeeded,
    AuthenticatedAbsent,
    InProgress,
    DefiniteFailure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderCall {
    step: WorkloadRestartStep,
    mode: WorkloadRestartCommandMode,
    attempt_id: WorkloadExecutionAttemptId,
    dispatch_epoch: WorkloadRestartDispatchEpoch,
}

struct ScriptedProvider {
    outcomes: Mutex<VecDeque<ScriptedOutcome>>,
    calls: Mutex<Vec<ProviderCall>>,
    successor_race: Mutex<Option<Arc<DurableStore>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolutionFenceCall {
    provider_call_count: usize,
    source_attempt_id: WorkloadExecutionAttemptId,
    target_attempt_id: WorkloadExecutionAttemptId,
}

struct RecordingResolutionFence {
    provider: Arc<ScriptedProvider>,
    withdrawals: Mutex<Vec<ResolutionFenceCall>>,
    restorations: Mutex<Vec<ResolutionFenceCall>>,
    fail_next_restoration: AtomicBool,
}

impl RecordingResolutionFence {
    fn new(provider: Arc<ScriptedProvider>) -> Arc<Self> {
        Arc::new(Self {
            provider,
            withdrawals: Mutex::new(Vec::new()),
            restorations: Mutex::new(Vec::new()),
            fail_next_restoration: AtomicBool::new(false),
        })
    }

    fn with_restore_failure(provider: Arc<ScriptedProvider>) -> Arc<Self> {
        let fence = Self::new(provider);
        fence.fail_next_restoration.store(true, Ordering::Release);
        fence
    }

    fn withdrawals(&self) -> Vec<ResolutionFenceCall> {
        self.withdrawals
            .lock()
            .expect("resolution withdrawal log should be healthy")
            .clone()
    }

    fn restorations(&self) -> Vec<ResolutionFenceCall> {
        self.restorations
            .lock()
            .expect("resolution restoration log should be healthy")
            .clone()
    }
}

impl WorkloadRestartResolutionFence for RecordingResolutionFence {
    fn withdraw(&self, record: &WorkloadSagaRecord) -> Result<(), nimbus_core::Error> {
        let active = record
            .restart_state()
            .active()
            .expect("withdrawal should retain an active restart");
        self.withdrawals
            .lock()
            .expect("resolution withdrawal log should be healthy")
            .push(ResolutionFenceCall {
                provider_call_count: self.provider.calls().len(),
                source_attempt_id: active.admission().source_attempt_id().clone(),
                target_attempt_id: active.admission().attempt_id().clone(),
            });
        Ok(())
    }

    fn restore<'a>(
        &'a self,
        record: &'a WorkloadSagaRecord,
    ) -> WorkloadRestartResolutionFuture<'a> {
        Box::pin(async move {
            let completed = record
                .restart_state()
                .last_completed()
                .expect("restoration should retain completed restart evidence");
            self.restorations
                .lock()
                .expect("resolution restoration log should be healthy")
                .push(ResolutionFenceCall {
                    provider_call_count: self.provider.calls().len(),
                    source_attempt_id: completed.admission().source_attempt_id().clone(),
                    target_attempt_id: completed.admission().attempt_id().clone(),
                });
            if self.fail_next_restoration.swap(false, Ordering::AcqRel) {
                return Err(nimbus_core::Error::Internal(
                    "scripted resolution restoration failure".to_owned(),
                ));
            }
            Ok(())
        })
    }
}

impl ScriptedProvider {
    fn new(outcomes: impl IntoIterator<Item = ScriptedOutcome>) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
            successor_race: Mutex::new(None),
        })
    }

    fn with_successor_race(
        outcomes: impl IntoIterator<Item = ScriptedOutcome>,
        store: Arc<DurableStore>,
    ) -> Arc<Self> {
        Arc::new(Self {
            outcomes: Mutex::new(outcomes.into_iter().collect()),
            calls: Mutex::new(Vec::new()),
            successor_race: Mutex::new(Some(store)),
        })
    }

    fn invoke(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        mode: WorkloadRestartCommandMode,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        self.calls
            .lock()
            .expect("provider call log should be healthy")
            .push(ProviderCall {
                step: command.step(),
                mode,
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
            });
        let outcome = self
            .outcomes
            .lock()
            .expect("provider outcome queue should be healthy")
            .pop_front()
            .unwrap_or(ScriptedOutcome::InProgress);
        let outcome = match outcome {
            ScriptedOutcome::Succeeded => WorkloadRestartCommandOutcome::Succeeded {
                evidence: WorkloadRestartEvidenceDigest::sha256("scripted-success"),
            },
            ScriptedOutcome::AuthenticatedAbsent => {
                WorkloadRestartCommandOutcome::AuthenticatedAbsent {
                    evidence: WorkloadRestartEvidenceDigest::sha256("scripted-absence"),
                }
            }
            ScriptedOutcome::InProgress => WorkloadRestartCommandOutcome::InProgress {
                evidence: WorkloadRestartEvidenceDigest::sha256("scripted-progress"),
            },
            ScriptedOutcome::DefiniteFailure => WorkloadRestartCommandOutcome::DefiniteFailure {
                evidence: WorkloadRestartEvidenceDigest::sha256("scripted-failure"),
            },
        };
        let observation =
            WorkloadRestartProviderObservation::new(WorkloadRestartProviderObservationInput {
                command_id: command.command_id().clone(),
                transition_id: command.transition_id().clone(),
                generation: command.generation(),
                desired_digest: command.desired_digest(),
                request_id: command.request_id().clone(),
                source_attempt_id: command.source_attempt_id().clone(),
                attempt_id: command.attempt_id().clone(),
                restart_epoch: command.restart_epoch(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_selection: command.provider_selection().clone(),
                outcome,
            });
        if mode == WorkloadRestartCommandMode::Execute
            && let Some(store) = self
                .successor_race
                .lock()
                .expect("successor race should be healthy")
                .take()
        {
            let mut current = store
                .record
                .lock()
                .expect("durable store lock should be healthy");
            let WorkloadSagaIntentUpdate::Transition(next) = current
                .apply_intent(stopped_successor(&current))
                .expect("racing successor should durably veto issued work")
            else {
                panic!("racing successor must transition");
            };
            current
                .validate_successor(&next)
                .expect("racing successor must remain a valid CAS candidate");
            *current = *next;
        }
        Box::pin(async move { observation })
    }

    fn calls(&self) -> Vec<ProviderCall> {
        self.calls
            .lock()
            .expect("provider call log should be healthy")
            .clone()
    }
}

fn step_modes(calls: &[ProviderCall]) -> Vec<(WorkloadRestartStep, WorkloadRestartCommandMode)> {
    calls.iter().map(|call| (call.step, call.mode)).collect()
}

macro_rules! effect_capability {
    ($capability:ident) => {
        impl $capability for ScriptedProvider {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.invoke(command, WorkloadRestartCommandMode::Execute)
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.invoke(command, WorkloadRestartCommandMode::Inspect)
            }
        }
    };
}

macro_rules! inspection_capability {
    ($capability:ident) => {
        impl $capability for ScriptedProvider {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.invoke(command, WorkloadRestartCommandMode::Inspect)
            }
        }
    };
}

effect_capability!(RestartPublicationWithdrawalCapability);
effect_capability!(WorkloadExecutionQuiescenceCapability);
effect_capability!(WorkloadRestartPreparationCapability);
effect_capability!(NetworkRestartAttachmentCapability);
inspection_capability!(WorkloadRestartActivationPrerequisiteCapability);
effect_capability!(WorkloadRestartActivationCapability);
inspection_capability!(WorkloadRestartReadinessCapability);
effect_capability!(RestartPublicationCapability);
inspection_capability!(RestartPublicationObservationCapability);

fn admit_observed_record(label: &str, observed: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let request = WorkloadRestartAdmissionRequest::for_explicit(
        observed,
        label,
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit restart should validate");
    let WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(observed, &request).expect("restart should admit")
    else {
        panic!("new restart should transition");
    };
    *admitted
}

fn admitted_record(label: &str) -> WorkloadSagaRecord {
    let observed = test_support::restart_observed_record(label, WorkloadRestartPolicy::Never);
    admit_observed_record(label, &observed)
}

fn published_admitted_record(label: &str) -> WorkloadSagaRecord {
    let observed = provision_record(
        label,
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    admit_observed_record(label, &observed)
}

fn fixture_provider_reports() -> NetworkCapabilityRegistry {
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
    .expect("fixture provider reports should validate")
}

fn stopped_successor(record: &WorkloadSagaRecord) -> WorkloadSagaIntent {
    let generation = WorkloadGeneration::new(record.active_intent().generation().as_u64() + 1);
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let identity = WorkloadNetworkPlanIdentity::new(
        record.key().tenant_id().clone(),
        "restart-withdrawal-successor",
        NetworkResourceGeneration::new(generation.as_u64()),
    )
    .expect("successor network identity should validate");
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
    .expect("successor network content should validate");
    let network = WorkloadNetworkIntent::new(
        CompiledWorkloadNetworkPlan::from_content(content)
            .expect("successor compiled network plan should validate"),
    );
    WorkloadSagaIntent::new_with_restart_policy(
        record.active_intent().kind(),
        DesiredWorkloadState::Stopped,
        generation,
        record.active_intent().executable().clone(),
        record.active_intent().source().clone(),
        WorkloadRestartPolicy::Never,
        network,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        record.active_intent().admission().clone(),
    )
    .expect("stopped successor should validate")
}

fn driver_with_fence(
    store: Arc<DurableStore>,
    provider: Arc<ScriptedProvider>,
    resolution_fence: Arc<dyn WorkloadRestartResolutionFence>,
) -> WorkloadRestartDriver {
    let record = store.record();
    let selected = record
        .active_intent()
        .source()
        .execution_provider_id()
        .clone();
    let network_selection = record
        .active_intent()
        .network()
        .compiled_plan()
        .content()
        .capability_selection()
        .cloned();
    let registry = WorkloadRestartCapabilityRegistry::new([WorkloadRestartCapabilities::new(
        selected,
        network_selection.clone(),
        provider.clone(),
        provider.clone(),
        provider,
    )])
    .expect("one exact restart provider should register");
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
    let dispatcher = Arc::new(WorkloadRestartDispatcher::new(
        Arc::new(ExactSourceAuthority {
            source: record.active_intent().source().clone(),
        }),
        if network_selection.is_some() {
            fixture_provider_reports()
        } else {
            NetworkCapabilityRegistry::new([]).expect("empty provider reports should validate")
        },
        Arc::new(registry),
    ));
    WorkloadRestartDriver::new(coordinator, dispatcher, resolution_fence)
}

fn driver(store: Arc<DurableStore>, provider: Arc<ScriptedProvider>) -> WorkloadRestartDriver {
    driver_with_fence(
        store,
        provider,
        Arc::new(NoopWorkloadRestartResolutionFence),
    )
}

#[tokio::test]
async fn withheld_publication_skips_ingress_and_quiesces_first() {
    let admitted = admitted_record("driver-order");
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([
        ScriptedOutcome::Succeeded,
        ScriptedOutcome::InProgress,
        ScriptedOutcome::InProgress,
    ]);

    let run = driver(store, provider.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("bounded driver should return durable waiting truth");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Waiting);
    assert!(run.record().restart_state().active().is_some());
    assert_eq!(
        step_modes(&provider.calls()),
        vec![
            (
                WorkloadRestartStep::QuiesceExecution,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::PrepareExecution,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::PrepareExecution,
                WorkloadRestartCommandMode::Inspect,
            ),
        ]
    );
}

#[tokio::test]
async fn definite_failure_stops_later_commands() {
    let admitted = admitted_record("driver-failure");
    let key = admitted.key().clone();
    let store = DurableStore::new(admitted);
    let provider = ScriptedProvider::new([ScriptedOutcome::DefiniteFailure]);

    let run = driver(store, provider.clone())
        .resume(&key, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("definite failure should return durable truth");

    assert_eq!(
        run.disposition(),
        WorkloadRestartRunDisposition::DefiniteFailure
    );
    assert_eq!(provider.calls().len(), 1);
}

#[tokio::test]
async fn crash_after_claim_before_effect_inspects_only() {
    let admitted = admitted_record("driver-crash-claim");
    let request_id = admitted
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .request_id()
        .clone();
    let quiescence = admitted
        .advance_restart_without_effect(&request_id)
        .expect("withheld restart should enter quiescence");
    let WorkloadRestartDecision::Proposed(pending) =
        decide_restart_progress(&quiescence, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("quiescence should claim")
    else {
        panic!("quiescence should claim a provider command");
    };
    assert_eq!(
        pending.action_after_confirmation(),
        Some(WorkloadRestartSymbolicAction::StartExactAttempt)
    );
    let pending = pending.into_candidate();
    let key = pending.key().clone();
    let store = DurableStore::new(pending);
    let provider = ScriptedProvider::new([ScriptedOutcome::InProgress]);

    let run = driver(store, provider.clone())
        .resume(&key, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("fresh recovery should inspect and wait");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Waiting);
    assert_eq!(
        step_modes(&provider.calls()),
        vec![(
            WorkloadRestartStep::QuiesceExecution,
            WorkloadRestartCommandMode::Inspect,
        )]
    );
}

const COMPLETE_RESTART_STEPS: [WorkloadRestartStep; 9] = [
    WorkloadRestartStep::WithdrawPublication,
    WorkloadRestartStep::QuiesceExecution,
    WorkloadRestartStep::PrepareExecution,
    WorkloadRestartStep::AttachNetwork,
    WorkloadRestartStep::InspectActivationPrerequisites,
    WorkloadRestartStep::ActivateExecution,
    WorkloadRestartStep::InspectReadiness,
    WorkloadRestartStep::Publish,
    WorkloadRestartStep::ObservePublication,
];

const COMPLETE_RESTART_MODES: [WorkloadRestartCommandMode; 9] = [
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Inspect,
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Inspect,
    WorkloadRestartCommandMode::Execute,
    WorkloadRestartCommandMode::Inspect,
];

async fn completed_published_restart(label: &str) -> (WorkloadRestartRun, Arc<ScriptedProvider>) {
    let admitted = published_admitted_record(label);
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([ScriptedOutcome::Succeeded; 9]);

    let run = driver(store, provider.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("complete published restart should converge");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Completed);
    assert!(run.record().restart_state().active().is_none());
    let calls = provider.calls();
    assert_eq!(
        calls.iter().map(|call| call.step).collect::<Vec<_>>(),
        COMPLETE_RESTART_STEPS
    );
    assert_eq!(
        calls.iter().map(|call| call.mode).collect::<Vec<_>>(),
        COMPLETE_RESTART_MODES
    );
    (run, provider)
}

#[tokio::test]
async fn observe_publication_absence_republishes_before_exact_observation() {
    let admitted = published_admitted_record("observe-publication-absence");
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([ScriptedOutcome::Succeeded; 8].into_iter().chain([
        ScriptedOutcome::AuthenticatedAbsent,
        ScriptedOutcome::Succeeded,
        ScriptedOutcome::Succeeded,
    ]));

    let run = driver(store, provider.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("authenticated observation absence should republish before observing again");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Completed);
    let calls = provider.calls();
    assert_eq!(calls.len(), 11);
    assert_eq!(
        step_modes(&calls[8..]),
        vec![
            (
                WorkloadRestartStep::ObservePublication,
                WorkloadRestartCommandMode::Inspect,
            ),
            (
                WorkloadRestartStep::Publish,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::ObservePublication,
                WorkloadRestartCommandMode::Inspect,
            ),
        ]
    );
    assert_eq!(
        calls[8].dispatch_epoch.checked_next(),
        Some(calls[9].dispatch_epoch)
    );
    assert_eq!(
        calls[10].dispatch_epoch,
        WorkloadRestartDispatchEpoch::new(0)
    );
    assert_eq!(calls[8].attempt_id, calls[9].attempt_id);
    assert_eq!(calls[9].attempt_id, calls[10].attempt_id);
}

#[tokio::test]
async fn publication_withdrawal_precedes_execution_quiescence() {
    let (_, provider) = completed_published_restart("restart-withdrawal-order").await;
    let calls = provider.calls();

    assert_eq!(
        step_modes(&calls[..2]),
        vec![
            (
                WorkloadRestartStep::WithdrawPublication,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::QuiesceExecution,
                WorkloadRestartCommandMode::Execute,
            ),
        ]
    );
}

#[tokio::test]
async fn resolution_fence_spans_withdrawal_through_publication_observation() {
    let admitted = published_admitted_record("restart-resolution-fence-order");
    let source_attempt_id = admitted
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .source_attempt_id()
        .clone();
    let target_attempt_id = admitted
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .attempt_id()
        .clone();
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([ScriptedOutcome::Succeeded; 9]);
    let fence = RecordingResolutionFence::new(provider.clone());

    let run = driver_with_fence(store, provider, fence.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("published restart should converge with resolution fencing");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Completed);
    assert_eq!(
        fence.withdrawals(),
        vec![ResolutionFenceCall {
            provider_call_count: 0,
            source_attempt_id: source_attempt_id.clone(),
            target_attempt_id: target_attempt_id.clone(),
        }],
        "resolution must be fenced before the first provider withdrawal call"
    );
    assert_eq!(
        fence.restorations(),
        vec![ResolutionFenceCall {
            provider_call_count: COMPLETE_RESTART_STEPS.len(),
            source_attempt_id,
            target_attempt_id,
        }],
        "resolution must reopen only after durable publication observation"
    );
}

#[tokio::test]
async fn completed_restart_retries_resolution_restoration_without_provider_replay() {
    let admitted = published_admitted_record("restart-resolution-restore-retry");
    let key = admitted.key().clone();
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([ScriptedOutcome::Succeeded; 9]);
    let fence = RecordingResolutionFence::with_restore_failure(provider.clone());

    let first = driver_with_fence(store.clone(), provider.clone(), fence.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await;
    assert!(matches!(first, Err(WorkloadRestartRunError::Resolution(_))));
    assert!(store.record().restart_state().active().is_none());
    assert_eq!(provider.calls().len(), COMPLETE_RESTART_STEPS.len());

    let resumed = driver_with_fence(store, provider.clone(), fence.clone())
        .resume(&key, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("completed restart should retry the exact resolution release");

    assert_eq!(
        resumed.disposition(),
        WorkloadRestartRunDisposition::Waiting
    );
    assert_eq!(provider.calls().len(), COMPLETE_RESTART_STEPS.len());
    assert_eq!(fence.restorations().len(), 2);
    assert!(
        fence
            .restorations()
            .iter()
            .all(|call| call.provider_call_count == COMPLETE_RESTART_STEPS.len()),
        "release retry must not repeat any provider effect"
    );
}

#[tokio::test]
async fn failed_publication_withdrawal_keeps_resolution_fenced() {
    let admitted = published_admitted_record("restart-resolution-withdrawal-failure");
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::new([ScriptedOutcome::DefiniteFailure]);
    let fence = RecordingResolutionFence::new(provider.clone());

    let run = driver_with_fence(store, provider.clone(), fence.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("definite withdrawal failure should return durable truth");

    assert_eq!(
        run.disposition(),
        WorkloadRestartRunDisposition::DefiniteFailure
    );
    assert_eq!(provider.calls().len(), 1);
    assert_eq!(fence.withdrawals().len(), 1);
    assert!(
        fence.restorations().is_empty(),
        "failed withdrawal must not reopen logical service resolution"
    );
}

#[tokio::test]
async fn restart_retained_detach_precedes_attachment() {
    let (_, provider) = completed_published_restart("restart-retained-order").await;
    let calls = provider.calls();

    assert_eq!(
        calls
            .iter()
            .take(4)
            .map(|call| call.step)
            .collect::<Vec<_>>(),
        [
            WorkloadRestartStep::WithdrawPublication,
            WorkloadRestartStep::QuiesceExecution,
            WorkloadRestartStep::PrepareExecution,
            WorkloadRestartStep::AttachNetwork,
        ]
    );
    assert_eq!(calls[1].attempt_id, calls[3].attempt_id);
}

#[tokio::test]
async fn activation_waits_for_same_generation_attachment_and_pep() {
    let (_, provider) = completed_published_restart("restart-activation-prerequisites").await;
    let calls = provider.calls();

    assert_eq!(
        calls[3..6]
            .iter()
            .map(|call| (call.step, call.mode))
            .collect::<Vec<_>>(),
        [
            (
                WorkloadRestartStep::AttachNetwork,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::InspectActivationPrerequisites,
                WorkloadRestartCommandMode::Inspect,
            ),
            (
                WorkloadRestartStep::ActivateExecution,
                WorkloadRestartCommandMode::Execute,
            ),
        ]
    );
    assert!(
        calls[3..6]
            .iter()
            .all(|call| call.attempt_id == calls[3].attempt_id)
    );
}

#[tokio::test]
async fn readiness_binds_the_new_execution_attempt() {
    let (run, provider) = completed_published_restart("restart-readiness-attempt").await;
    let calls = provider.calls();
    let readiness = &calls[6];

    assert_eq!(readiness.step, WorkloadRestartStep::InspectReadiness);
    assert_eq!(readiness.mode, WorkloadRestartCommandMode::Inspect);
    assert_eq!(readiness.attempt_id, calls[3].attempt_id);
    assert_eq!(
        readiness.attempt_id,
        *run.record().restart_state().current_execution_attempt_id()
    );
}

#[tokio::test]
async fn publication_waits_for_new_attempt_readiness() {
    let (_, provider) = completed_published_restart("restart-publication-readiness").await;
    let calls = provider.calls();

    assert_eq!(
        step_modes(&calls[6..9]),
        vec![
            (
                WorkloadRestartStep::InspectReadiness,
                WorkloadRestartCommandMode::Inspect,
            ),
            (
                WorkloadRestartStep::Publish,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::ObservePublication,
                WorkloadRestartCommandMode::Inspect,
            ),
        ]
    );
    assert!(
        calls[6..9]
            .iter()
            .all(|call| call.attempt_id == calls[6].attempt_id)
    );
}

#[tokio::test]
async fn withdrawal_after_admission_vetoes_unissued_command() {
    let admitted = admitted_record("restart-unissued-withdrawal");
    let WorkloadSagaIntentUpdate::Transition(withdrawn) =
        admitted.apply_intent(stopped_successor(&admitted)).unwrap()
    else {
        panic!("successor should withdraw the unissued restart");
    };
    assert_eq!(withdrawn.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert!(withdrawn.restart_state().active().is_none());
    let store = DurableStore::new(*withdrawn);
    let provider = ScriptedProvider::new([]);

    let run = driver(store, provider.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("stale admitted work should adopt the durable withdrawal");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Completed);
    assert_eq!(run.record().phase(), WorkloadSagaPhase::WithdrawalCommitted);
    assert!(run.record().successor_intent().is_some());
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn successor_after_effect_before_result_cas_allows_inspection_only() {
    let admitted = admitted_record("restart-ambiguous-withdrawal");
    let store = DurableStore::new(admitted.clone());
    let provider = ScriptedProvider::with_successor_race(
        [ScriptedOutcome::Succeeded, ScriptedOutcome::Succeeded],
        store.clone(),
    );

    let run = driver(store, provider.clone())
        .drive_admitted(admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
        .await
        .expect("result-CAS conflict should adopt the successor veto and inspect exact work");

    assert_eq!(run.disposition(), WorkloadRestartRunDisposition::Waiting);
    let calls = provider.calls();
    assert_eq!(
        step_modes(&calls),
        vec![
            (
                WorkloadRestartStep::QuiesceExecution,
                WorkloadRestartCommandMode::Execute,
            ),
            (
                WorkloadRestartStep::QuiesceExecution,
                WorkloadRestartCommandMode::Inspect,
            ),
        ]
    );
    assert_eq!(calls[0].attempt_id, calls[1].attempt_id);
    assert!(run.record().successor_intent().is_some());
    assert!(matches!(
        run.record()
            .restart_state()
            .active()
            .expect("issued evidence must remain durable")
            .disposition(),
        nimbus_workloads::WorkloadRestartDisposition::SuccessorVetoed {
            result: WorkloadRestartEffectResult::Succeeded { .. },
            ..
        }
    ));
}
