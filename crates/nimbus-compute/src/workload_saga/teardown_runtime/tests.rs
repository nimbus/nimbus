use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use nimbus_network::NetworkProviderId;
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadSagaPhase, WorkloadTeardownCommandMode,
};
use tokio::sync::{Notify, Semaphore};

use super::*;
use crate::workload_saga::recovery::tests::{evidence, teardown_record, teardown_success_evidence};
use crate::workload_saga::teardown_test_support::{
    DurableTeardownStore, RecordingTeardownProvider, StaticSourceAuthority,
    TeardownProviderBehavior, provider_reports, teardown_capabilities,
};
use crate::workload_saga::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    WorkloadExecutionTeardownCapabilities, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};

fn initial(label: &str) -> nimbus_workloads::WorkloadSagaRecord {
    teardown_record(label, WorkloadSagaPhase::WithdrawalCommitted)
}

fn assert_send_sync<T: Send + Sync>() {}

#[tokio::test]
async fn cancellation_before_runtime_submission_makes_zero_calls() {
    assert_send_sync::<WorkloadTeardownRuntime>();
    let record = initial("teardown-runtime-cancel-before");
    let store = DurableTeardownStore::with_record(record.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let runtime = WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(teardown_capabilities(provider.clone())),
    );
    let cancellation = WorkloadTeardownCancellationToken::new();
    cancellation.cancel();

    assert!(matches!(
        runtime.submit(record.key().clone(), &cancellation).await,
        Err(WorkloadTeardownSubmissionError::Cancelled)
    ));
    assert_eq!(store.counts(), (0, 0));
    assert!(provider.calls().is_empty());
}

struct BlockingIngress {
    started: Notify,
    release: Notify,
    blocked_once: AtomicBool,
    execute_calls: AtomicUsize,
    inspect_calls: AtomicUsize,
}

impl BlockingIngress {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            started: Notify::new(),
            release: Notify::new(),
            blocked_once: AtomicBool::new(false),
            execute_calls: AtomicUsize::new(0),
            inspect_calls: AtomicUsize::new(0),
        })
    }

    fn observation(
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderObservation {
        let outcome = match command.mode() {
            WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::Succeeded(Box::new(teardown_success_evidence(
                    command.step(),
                    command.subjects(),
                ))),
            ),
            WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
                crate::workload_saga::WorkloadTeardownInspectOutcome::NotCompleted(evidence(
                    "blocking-ingress-not-completed",
                )),
            ),
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }
}

impl FinalIngressWithdrawalCapability for BlockingIngress {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            self.execute_calls.fetch_add(1, Ordering::AcqRel);
            if !self.blocked_once.swap(true, Ordering::AcqRel) {
                self.started.notify_one();
                self.release.notified().await;
            }
            Self::observation(command)
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            self.inspect_calls.fetch_add(1, Ordering::AcqRel);
            Self::observation(command)
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_direct_winner_prevents_not_completed_retry_overlap() {
    let record = initial("teardown-runtime-singleflight");
    let store = DurableTeardownStore::with_record(record.clone());
    let later = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let ingress = BlockingIngress::new();
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            later.clone(),
            later.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
            later.clone(),
            later.clone(),
        )],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            ingress.clone(),
        )],
    )
    .expect("blocking runtime capability registry is valid");
    let runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(capabilities),
    ));
    let joined = Arc::new(Semaphore::new(0));
    runtime.install_test_retained_join_boundary(Arc::clone(&joined));

    let first_runtime = Arc::clone(&runtime);
    let first_key = record.key().clone();
    let first = tokio::spawn(async move {
        first_runtime
            .submit(first_key, &WorkloadTeardownCancellationToken::new())
            .await
    });
    ingress.started.notified().await;

    let second_runtime = Arc::clone(&runtime);
    let second_key = record.key().clone();
    let second = tokio::spawn(async move {
        second_runtime
            .submit(second_key, &WorkloadTeardownCancellationToken::new())
            .await
    });
    joined
        .acquire()
        .await
        .expect("the duplicate waiter should join retained work")
        .forget();

    assert_eq!(ingress.execute_calls.load(Ordering::Acquire), 1);
    assert_eq!(ingress.inspect_calls.load(Ordering::Acquire), 0);
    let pending = store.record();
    let claim = pending
        .teardown_disposition()
        .and_then(nimbus_workloads::WorkloadTeardownDisposition::claim)
        .expect("the blocked direct winner retains its exact pending claim");
    assert_eq!(claim.dispatch_epoch().as_u64(), 0);

    ingress.release.notify_one();
    let first = first
        .await
        .expect("first waiter joins")
        .expect("first waiter observes convergence");
    let second = second
        .await
        .expect("second waiter joins")
        .expect("second waiter observes the same convergence");
    assert_eq!(first, second);
    assert_eq!(first.record().phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(ingress.execute_calls.load(Ordering::Acquire), 1);
    assert_eq!(ingress.inspect_calls.load(Ordering::Acquire), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_during_waiter_registration_prevents_submission() {
    let record = initial("teardown-runtime-cancel-registration");
    let store = DurableTeardownStore::with_record(record.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(teardown_capabilities(provider.clone())),
    ));
    let cancellation = WorkloadTeardownCancellationToken::new();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    cancellation.install_test_registration_boundary(Arc::clone(&entered), Arc::clone(&release));

    let waiter_runtime = Arc::clone(&runtime);
    let waiter_cancellation = cancellation.clone();
    let key = record.key().clone();
    let waiter =
        tokio::spawn(async move { waiter_runtime.submit(key, &waiter_cancellation).await });

    let entered_wait = Arc::clone(&entered);
    tokio::task::spawn_blocking(move || entered_wait.wait())
        .await
        .expect("registration entry barrier joins");
    cancellation.cancel();
    let release_wait = Arc::clone(&release);
    tokio::task::spawn_blocking(move || release_wait.wait())
        .await
        .expect("registration release barrier joins");

    assert!(matches!(
        waiter.await.expect("registration waiter joins"),
        Err(WorkloadTeardownSubmissionError::Cancelled)
    ));
    assert_eq!(store.counts(), (0, 0));
    assert!(provider.calls().is_empty());
}

#[tokio::test]
async fn cancellation_after_claim_detaches_only_waiter() {
    let record = initial("teardown-runtime-cancel-after");
    let store = DurableTeardownStore::with_record(record.clone());
    let later = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let ingress = BlockingIngress::new();
    let capabilities = WorkloadTeardownCapabilityRegistry::new(
        [NetworkAttachmentTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-attachment"),
            later.clone(),
            later.clone(),
        )],
        [WorkloadExecutionTeardownCapabilities::new(
            WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
            later.clone(),
            later.clone(),
        )],
        [IngressTeardownCapabilities::new(
            NetworkProviderId::for_registration_key("fixture-ingress"),
            ingress.clone(),
        )],
    )
    .expect("blocking runtime capability registry is valid");
    let runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::new(WorkloadSagaCoordinator::new(store.clone())),
        StaticSourceAuthority::exact(&record),
        provider_reports(),
        Arc::new(capabilities),
    ));
    let cancellation = WorkloadTeardownCancellationToken::new();
    let waiter_runtime = Arc::clone(&runtime);
    let waiter_cancellation = cancellation.clone();
    let key = record.key().clone();
    let waiter =
        tokio::spawn(async move { waiter_runtime.submit(key, &waiter_cancellation).await });

    ingress.started.notified().await;
    cancellation.cancel();
    assert!(matches!(
        waiter.await.expect("waiter task joins"),
        Err(WorkloadTeardownSubmissionError::Cancelled)
    ));
    ingress.release.notify_one();

    let completed = runtime
        .submit(
            record.key().clone(),
            &WorkloadTeardownCancellationToken::new(),
        )
        .await
        .expect("a later exact-key waiter observes retained convergence");
    assert_eq!(completed.record().phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(ingress.execute_calls.load(Ordering::Acquire), 1);
}
