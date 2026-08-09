use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_network::{NetworkCapabilityRegistry, NetworkProviderId};
use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceResourceVersion, WorkloadSagaPhase,
    WorkloadTeardownDecision, WorkloadTeardownStep,
};

use super::*;
use crate::workload_saga::recovery::tests::{teardown_record, teardown_success_evidence};
use crate::workload_saga::teardown_decision::materialize_teardown_candidate;
use crate::workload_saga::teardown_test_support::{
    DurableTeardownStore, RecordingTeardownProvider, StaticSourceAuthority,
    TeardownProviderBehavior, provider_reports, teardown_capabilities,
};
use crate::workload_saga::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, WorkloadSagaCoordinator, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownCapabilityRegistry, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome,
};

fn initial(label: &str) -> nimbus_workloads::WorkloadSagaRecord {
    teardown_record(label, WorkloadSagaPhase::WithdrawalCommitted)
}

async fn confirmed_execute(
    record: &nimbus_workloads::WorkloadSagaRecord,
) -> ConfirmedWorkloadTeardownTransition {
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = record.decide_teardown().expect("withdrawal is reducible")
    else {
        panic!("withdrawal fixture must require a claim");
    };
    let candidate = materialize_teardown_candidate(record, &proposed).expect("claim materializes");
    let store = DurableTeardownStore::with_record(record.clone());
    WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(record, candidate)
        .await
        .expect("claim confirmation succeeds")
}

fn changed_source(
    record: &nimbus_workloads::WorkloadSagaRecord,
) -> WorkloadProvisionSourceEvidence {
    let source = record.active_intent().source();
    WorkloadProvisionSourceEvidence::standalone_sandbox(
        source.source_identity().clone(),
        WorkloadProvisionSourceGeneration::new(99),
        WorkloadProvisionSourceResourceVersion::new("teardown-source-drift")
            .expect("fixture source version is valid"),
        record.active_intent().executable().content_digest(),
        source.attachment_provider_id().clone(),
        source.execution_provider_id().clone(),
    )
    .expect("changed source evidence is valid")
}

#[tokio::test]
async fn execute_reauthenticates_source_and_provider_reports() {
    let record = initial("teardown-dispatch-exact");
    let confirmed = confirmed_execute(&record).await;
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let dispatcher = WorkloadTeardownDispatcher::new(
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(teardown_capabilities(provider.clone())),
    );

    let result = dispatcher
        .dispatch_confirmed(&confirmed)
        .await
        .expect("exact source and reports authorize one effect");

    assert!(result.is_some());
    assert_eq!(provider.calls().len(), 1);
}

#[tokio::test]
async fn stale_execute_evidence_makes_zero_capability_calls() {
    let record = initial("teardown-dispatch-stale");
    let confirmed = confirmed_execute(&record).await;
    let source = StaticSourceAuthority::exact(&record);
    source.replace(changed_source(&record));
    let source_provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let source_dispatcher = WorkloadTeardownDispatcher::new(
        source,
        provider_reports(),
        Arc::new(teardown_capabilities(source_provider.clone())),
    );
    assert!(matches!(
        source_dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadTeardownDispatchError::CurrentSourceMismatch { .. })
    ));
    assert!(source_provider.calls().is_empty());

    let report_provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let report_dispatcher = WorkloadTeardownDispatcher::new(
        StaticSourceAuthority::exact(&record),
        NetworkCapabilityRegistry::new([]).expect("empty reports are valid"),
        Arc::new(teardown_capabilities(report_provider.clone())),
    );
    assert!(matches!(
        report_dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadTeardownDispatchError::ProviderSelection(_))
    ));
    assert!(report_provider.calls().is_empty());
}

#[tokio::test]
async fn inspection_remains_available_after_source_and_report_drift() {
    let record = initial("teardown-dispatch-inspect-drift");
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = record.decide_teardown().expect("withdrawal is reducible")
    else {
        panic!("withdrawal fixture must require a claim");
    };
    let pending = materialize_teardown_candidate(&record, &proposed).expect("claim materializes");
    let store = DurableTeardownStore::with_record(pending.clone());
    let coordinator = WorkloadSagaCoordinator::new(store);
    let confirmed = coordinator
        .inspect_confirmed_teardown(pending.key())
        .await
        .expect("recovery persists inspection");
    let source = StaticSourceAuthority::exact(&pending);
    source.replace(changed_source(&pending));
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let dispatcher = WorkloadTeardownDispatcher::new(
        source,
        NetworkCapabilityRegistry::new([]).expect("empty reports are valid"),
        Arc::new(teardown_capabilities(provider.clone())),
    );

    let result = dispatcher
        .dispatch_confirmed(&confirmed)
        .await
        .expect("inspection survives current evidence drift");

    assert!(result.is_some());
    assert_eq!(provider.calls().len(), 1);
    assert_eq!(
        provider.calls()[0].mode,
        nimbus_workloads::WorkloadTeardownCommandMode::Inspect
    );
}

struct CrossedIngressCapability {
    calls: AtomicUsize,
}

impl FinalIngressWithdrawalCapability for CrossedIngressCapability {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let outcome = WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(
                    teardown_success_evidence(command.step(), command.subjects()).into(),
                ),
            );
            let mut observation =
                WorkloadTeardownProviderObservation::for_command(command, outcome);
            observation.cross_confirmed_revision_for_test();
            observation
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        self.execute(command)
    }
}

#[tokio::test]
async fn crossed_provider_observation_fails_before_result_cas() {
    let record = initial("teardown-dispatch-crossed");
    let confirmed = confirmed_execute(&record).await;
    let capability = Arc::new(CrossedIngressCapability {
        calls: AtomicUsize::new(0),
    });
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [],
        [],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            capability.clone(),
        )],
    )
    .expect("crossed fixture registry is valid");
    let dispatcher = WorkloadTeardownDispatcher::new(
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(capabilities),
    );

    assert!(matches!(
        dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadTeardownDispatchError::CrossedProviderObservation)
    ));
    assert_eq!(capability.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        confirmed.confirmed_record().expect("durable claim").phase(),
        record.phase()
    );
    assert_eq!(
        confirmed.command().expect("execute command").step(),
        WorkloadTeardownStep::WithdrawPublication
    );
}

struct CrossedLocatorIngressCapability {
    calls: AtomicUsize,
    locator: nimbus_workloads::WorkloadExecutionReference,
}

impl FinalIngressWithdrawalCapability for CrossedLocatorIngressCapability {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::AcqRel);
            let outcome = WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(
                    teardown_success_evidence(command.step(), command.subjects()).into(),
                ),
            );
            let mut observation =
                WorkloadTeardownProviderObservation::for_command(command, outcome);
            observation.cross_execution_locator_for_test(self.locator.clone());
            observation
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        self.execute(command)
    }
}

#[tokio::test]
async fn crossed_execution_locator_fails_before_result_cas() {
    let record = initial("teardown-dispatch-crossed-locator");
    let confirmed = confirmed_execute(&record).await;
    let other = initial("teardown-dispatch-other-locator");
    let other_confirmed = confirmed_execute(&other).await;
    let locator = other_confirmed
        .command()
        .expect("other execute command")
        .execution_locator()
        .clone();
    let capability = Arc::new(CrossedLocatorIngressCapability {
        calls: AtomicUsize::new(0),
        locator,
    });
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [],
        [],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            capability.clone(),
        )],
    )
    .expect("crossed fixture registry is valid");
    let dispatcher = WorkloadTeardownDispatcher::new(
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(capabilities),
    );

    assert!(matches!(
        dispatcher.dispatch_confirmed(&confirmed).await,
        Err(WorkloadTeardownDispatchError::CrossedProviderObservation)
    ));
    assert_eq!(capability.calls.load(Ordering::Acquire), 1);
    assert_eq!(
        confirmed.confirmed_record().expect("durable claim").phase(),
        record.phase()
    );
}
