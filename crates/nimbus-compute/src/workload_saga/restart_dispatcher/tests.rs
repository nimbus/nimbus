use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::TenantId;
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadGeneration, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceIdentity, WorkloadRestartCandidatePage,
    WorkloadRestartCandidatePageRequest, WorkloadRestartDispatchEpoch, WorkloadRestartEpoch,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaKey, WorkloadSagaPage,
    WorkloadSagaPageRequest, WorkloadSagaStore, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest,
};

use super::*;
use crate::workload_saga::restart_provider::{
    NetworkRestartAttachmentCapability, RestartPublicationCapability,
    RestartPublicationObservationCapability, RestartPublicationWithdrawalCapability,
    WorkloadExecutionQuiescenceCapability, WorkloadRestartActivationCapability,
    WorkloadRestartActivationPrerequisiteCapability, WorkloadRestartCapabilities,
    WorkloadRestartCapabilityFuture, WorkloadRestartPreparationCapability,
    WorkloadRestartProviderObservation, WorkloadRestartProviderObservationInput,
    WorkloadRestartReadinessCapability,
};
use crate::workload_saga::{
    ConfirmedWorkloadRestartCommand, WorkloadRestartCommandOutcome, WorkloadRestartDecision,
    WorkloadRestartSymbolicAction, decide_restart_admission, decide_restart_progress, test_support,
};

struct TestStore {
    cas: Mutex<VecDeque<WorkloadSagaCommit>>,
}

impl TestStore {
    fn new(cas: impl IntoIterator<Item = WorkloadSagaCommit>) -> Arc<Self> {
        Arc::new(Self {
            cas: Mutex::new(cas.into_iter().collect()),
        })
    }
}

impl WorkloadSagaStore for TestStore {
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
        Box::pin(async move {
            self.cas
                .lock()
                .expect("test CAS queue should be healthy")
                .pop_front()
                .ok_or(WorkloadSagaStoreError::Unavailable)
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

struct SourceAuthority {
    source: Option<WorkloadProvisionSourceEvidence>,
    calls: AtomicUsize,
}

impl SourceAuthority {
    fn matching(source: WorkloadProvisionSourceEvidence) -> Arc<Self> {
        Arc::new(Self {
            source: Some(source),
            calls: AtomicUsize::new(0),
        })
    }

    fn unavailable() -> Arc<Self> {
        Arc::new(Self {
            source: None,
            calls: AtomicUsize::new(0),
        })
    }
}

impl WorkloadProvisionSourceAuthority for SourceAuthority {
    fn current_source<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
        _identity: &'a WorkloadProvisionSourceIdentity,
    ) -> super::super::WorkloadProvisionSourceFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.source
                .clone()
                .ok_or(WorkloadProvisionSourceAuthorityError::Unavailable)
        })
    }
}

#[derive(Default)]
struct CountingProvider {
    execute: AtomicUsize,
    inspect: AtomicUsize,
    observation: Mutex<Option<WorkloadRestartProviderObservation>>,
}

impl CountingProvider {
    fn observing(observation: WorkloadRestartProviderObservation) -> Arc<Self> {
        Arc::new(Self {
            observation: Mutex::new(Some(observation)),
            ..Self::default()
        })
    }

    fn outcome(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        execute: bool,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        if execute {
            self.execute.fetch_add(1, Ordering::SeqCst);
        } else {
            self.inspect.fetch_add(1, Ordering::SeqCst);
        }
        let observation = self
            .observation
            .lock()
            .expect("provider observation lock should be healthy")
            .clone()
            .unwrap_or_else(|| successful_observation(command));
        Box::pin(async move { observation })
    }
}

fn successful_observation_input(
    command: &ConfirmedWorkloadRestartCommand,
) -> WorkloadRestartProviderObservationInput {
    WorkloadRestartProviderObservationInput {
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
        outcome: WorkloadRestartCommandOutcome::Succeeded {
            evidence: nimbus_workloads::WorkloadRestartEvidenceDigest::sha256("counting-provider"),
        },
    }
}

fn successful_observation(
    command: &ConfirmedWorkloadRestartCommand,
) -> WorkloadRestartProviderObservation {
    WorkloadRestartProviderObservation::new(successful_observation_input(command))
}

macro_rules! effect_capability {
    ($capability:ident) => {
        impl $capability for CountingProvider {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.outcome(command, true)
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.outcome(command, false)
            }
        }
    };
}

macro_rules! inspection_capability {
    ($capability:ident) => {
        impl $capability for CountingProvider {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                self.outcome(command, false)
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

fn pending_command(label: &str) -> (WorkloadSagaRecord, ProposedWorkloadRestartTransition) {
    let observed = test_support::restart_observed_record(label, WorkloadRestartPolicy::Never);
    let request = super::super::WorkloadRestartAdmissionRequest::for_explicit(
        &observed,
        label,
        WorkloadRestartNotBeforeUnixMillis::new(0),
    )
    .expect("explicit restart request should validate");
    let super::super::WorkloadRestartAdmissionDecision::Transition(admitted) =
        decide_restart_admission(&observed, &request).expect("restart should admit")
    else {
        panic!("new restart should transition");
    };
    let WorkloadRestartDecision::Proposed(withdrawal) =
        decide_restart_progress(&admitted, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("requested restart should reduce")
    else {
        panic!("requested restart should enter withdrawal");
    };
    let withdrawal = withdrawal.into_candidate();
    let WorkloadRestartDecision::Proposed(pending) =
        decide_restart_progress(&withdrawal, WorkloadRestartNotBeforeUnixMillis::new(0))
            .expect("withdrawal should claim a command")
    else {
        panic!("withdrawal should produce a command claim");
    };
    assert_eq!(
        pending.action_after_confirmation(),
        Some(WorkloadRestartSymbolicAction::StartExactAttempt)
    );
    (withdrawal, pending)
}

async fn confirmed(
    label: &str,
    replay: bool,
) -> (ConfirmedWorkloadRestartTransition, WorkloadSagaRecord) {
    let (loaded, proposed) = pending_command(label);
    let store = if replay {
        TestStore::new([WorkloadSagaCommit::Unchanged, WorkloadSagaCommit::Applied])
    } else {
        TestStore::new([WorkloadSagaCommit::Applied])
    };
    let transition = WorkloadSagaCoordinator::new(store)
        .claim_restart_command(&loaded, &proposed)
        .await
        .expect("restart command should confirm");
    (transition, loaded)
}

fn registry(
    provider_id: WorkloadExecutionProviderId,
    provider: Arc<CountingProvider>,
) -> Arc<WorkloadRestartCapabilityRegistry> {
    Arc::new(
        WorkloadRestartCapabilityRegistry::new([WorkloadRestartCapabilities::new(
            provider_id,
            None,
            provider.clone(),
            provider.clone(),
            provider,
        )])
        .expect("one exact restart provider should register"),
    )
}

fn empty_reports() -> NetworkCapabilityRegistry {
    NetworkCapabilityRegistry::new([]).expect("empty reports should validate")
}

#[tokio::test]
async fn restart_dispatcher_execute_rechecks_source_but_inspection_survives_source_drift() {
    let (execute, _) = confirmed("execute-freshness", false).await;
    let provider = Arc::new(CountingProvider::default());
    let unavailable = SourceAuthority::unavailable();
    let dispatcher = WorkloadRestartDispatcher::new(
        unavailable.clone(),
        empty_reports(),
        registry(
            execute.command().unwrap().provider_selection().clone(),
            provider.clone(),
        ),
    );

    assert!(matches!(
        dispatcher.dispatch_confirmed(&execute).await,
        Err(WorkloadRestartDispatchError::Source(
            WorkloadProvisionSourceAuthorityError::Unavailable
        ))
    ));
    assert_eq!(provider.execute.load(Ordering::SeqCst), 0);

    let (inspection, _) = confirmed("inspection-source-drift", true).await;
    let inspection_provider = Arc::new(CountingProvider::default());
    let inspection_dispatcher = WorkloadRestartDispatcher::new(
        unavailable.clone(),
        empty_reports(),
        registry(
            inspection.command().unwrap().provider_selection().clone(),
            inspection_provider.clone(),
        ),
    );
    inspection_dispatcher
        .dispatch_confirmed(&inspection)
        .await
        .expect("inspection should survive source drift")
        .expect("inspection should return an exact result");

    assert_eq!(inspection_provider.inspect.load(Ordering::SeqCst), 1);
    assert_eq!(unavailable.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn restart_dispatcher_has_no_first_available_capability_fallback() {
    let (confirmed, loaded) = confirmed("dispatcher-no-fallback", false).await;
    let provider = Arc::new(CountingProvider::default());
    let other = nimbus_workloads::WorkloadExecutionProviderId::for_registration_key(
        "other-restart-provider",
    );
    let source = SourceAuthority::matching(loaded.active_intent().source().clone());
    let dispatcher =
        WorkloadRestartDispatcher::new(source, empty_reports(), registry(other, provider.clone()));

    assert!(matches!(
        dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadRestartDispatchError::Capability(
            WorkloadRestartCapabilityRegistryError::MissingProviderSelection { .. }
        ))
    ));
    assert_eq!(provider.execute.load(Ordering::SeqCst), 0);
    assert_eq!(provider.inspect.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn old_attempt_provider_observation_is_rejected_before_result() {
    let (confirmed, loaded) = confirmed("dispatcher-crossed-attempt", false).await;
    let old_attempt = loaded
        .restart_state()
        .current_execution_attempt_id()
        .clone();
    assert_ne!(old_attempt, *confirmed.command().unwrap().attempt_id());
    let mut observation = successful_observation_input(confirmed.command().unwrap());
    observation.attempt_id = old_attempt;
    let provider =
        CountingProvider::observing(WorkloadRestartProviderObservation::new(observation));
    let source = SourceAuthority::matching(loaded.active_intent().source().clone());
    let dispatcher = WorkloadRestartDispatcher::new(
        source,
        empty_reports(),
        registry(
            confirmed.command().unwrap().provider_selection().clone(),
            provider.clone(),
        ),
    );

    assert!(matches!(
        dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadRestartDispatchError::CrossedProviderObservation)
    ));
    assert_eq!(provider.execute.load(Ordering::SeqCst), 1);
    assert_eq!(provider.inspect.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn every_restart_provider_callback_fence_is_checked_before_result() {
    let (confirmation, loaded) = confirmed("dispatcher-callback-fences", false).await;
    let command = confirmation
        .command()
        .expect("restart command should exist");
    let (crossed_confirmation, _) = confirmed("dispatcher-callback-fences-crossed", false).await;
    let crossed = crossed_confirmation
        .command()
        .expect("crossed restart command should exist");

    let mut cases = Vec::new();

    let mut input = successful_observation_input(command);
    input.command_id = crossed.command_id().clone();
    cases.push(("command ID", input));

    let mut input = successful_observation_input(command);
    input.transition_id = crossed.transition_id().clone();
    cases.push(("transition ID", input));

    let mut input = successful_observation_input(command);
    input.generation = WorkloadGeneration::new(command.generation().as_u64() + 1);
    cases.push(("desired generation", input));

    let mut input = successful_observation_input(command);
    input.desired_digest = crossed.desired_digest();
    cases.push(("desired digest", input));

    let mut input = successful_observation_input(command);
    input.request_id = crossed.request_id().clone();
    cases.push(("request ID", input));

    let mut input = successful_observation_input(command);
    input.source_attempt_id = crossed.source_attempt_id().clone();
    cases.push(("source attempt", input));

    let mut input = successful_observation_input(command);
    input.attempt_id = crossed.attempt_id().clone();
    cases.push(("target attempt", input));

    let mut input = successful_observation_input(command);
    input.restart_epoch = WorkloadRestartEpoch::new(command.restart_epoch().as_u64() + 1);
    cases.push(("restart epoch", input));

    let mut input = successful_observation_input(command);
    input.dispatch_epoch = WorkloadRestartDispatchEpoch::new(command.dispatch_epoch().as_u64() + 1);
    cases.push(("dispatch epoch", input));

    let mut input = successful_observation_input(command);
    input.provider_selection =
        WorkloadExecutionProviderId::for_registration_key("crossed-restart-provider");
    cases.push(("provider selection", input));

    for (label, input) in cases {
        let provider = CountingProvider::observing(WorkloadRestartProviderObservation::new(input));
        let source = SourceAuthority::matching(loaded.active_intent().source().clone());
        let dispatcher = WorkloadRestartDispatcher::new(
            source,
            empty_reports(),
            registry(command.provider_selection().clone(), provider.clone()),
        );

        assert!(
            matches!(
                dispatcher.dispatch_confirmed(&confirmation).await,
                Err(WorkloadRestartDispatchError::CrossedProviderObservation)
            ),
            "a crossed {label} callback must fail before its result reaches the saga reducer"
        );
        assert_eq!(provider.execute.load(Ordering::SeqCst), 1, "{label}");
        assert_eq!(provider.inspect.load(Ordering::SeqCst), 0, "{label}");
    }

    let provider = CountingProvider::observing(successful_observation(command));
    let source = SourceAuthority::matching(loaded.active_intent().source().clone());
    let dispatcher = WorkloadRestartDispatcher::new(
        source,
        empty_reports(),
        registry(command.provider_selection().clone(), provider),
    );
    assert!(
        dispatcher
            .dispatch_confirmed(&confirmation)
            .await
            .expect("an exact callback should authenticate")
            .is_some()
    );
}
