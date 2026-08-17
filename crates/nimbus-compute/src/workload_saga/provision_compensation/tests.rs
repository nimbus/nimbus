use std::sync::Arc;

use nimbus_workloads::{
    WorkloadActivationIntent, WorkloadProvisionStep, WorkloadPublicationIntent, WorkloadSagaPhase,
    WorkloadSagaStore, WorkloadTeardownStep,
};

use super::*;
use crate::workload_saga::WorkloadTeardownRunDisposition;
use crate::workload_saga::recovery::tests::provision_record;
use crate::workload_saga::teardown_test_support::{
    CasFault, DurableTeardownStore, RecordingTeardownProvider, StaticSourceAuthority,
    TeardownProviderBehavior, provider_reports, teardown_capabilities,
};
use crate::workload_saga::test_support::failed_provision_record;

fn compensator(
    store: Arc<DurableTeardownStore>,
    source: &WorkloadSagaRecord,
    provider: Arc<RecordingTeardownProvider>,
) -> WorkloadProvisionCompensator {
    let saga_store: Arc<dyn WorkloadSagaStore> = store;
    let coordinator = Arc::new(WorkloadSagaCoordinator::new(saga_store));
    let source_authority = StaticSourceAuthority::exact(source);
    let runtime = Arc::new(WorkloadTeardownRuntime::new(
        Arc::clone(&coordinator),
        source_authority,
        provider_reports(),
        Arc::new(teardown_capabilities(provider)),
    ));
    WorkloadProvisionCompensator::new(coordinator, runtime)
}

#[tokio::test]
async fn eight_provision_failures_compensate_only_proven_resources_in_reverse_order() {
    let cases = [
        (WorkloadProvisionStep::ReserveNetwork, vec![]),
        (
            WorkloadProvisionStep::PrepareWorkload,
            vec![WorkloadTeardownStep::ReleaseNetwork],
        ),
        (
            WorkloadProvisionStep::AttachNetwork,
            vec![
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            vec![
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
        (
            WorkloadProvisionStep::ActivateWorkload,
            vec![
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            vec![
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
        (
            WorkloadProvisionStep::Publish,
            vec![
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
        (
            WorkloadProvisionStep::ObservePublication,
            vec![
                WorkloadTeardownStep::WithdrawPublication,
                WorkloadTeardownStep::DrainExecution,
                WorkloadTeardownStep::StopExecution,
                WorkloadTeardownStep::DetachNetwork,
                WorkloadTeardownStep::ReleaseNetwork,
            ],
        ),
    ];

    for (index, (failed_step, expected_steps)) in cases.into_iter().enumerate() {
        let failed = failed_provision_record(&format!("compensation-{index}"), failed_step);
        let store = DurableTeardownStore::with_record(failed.clone());
        let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
        let compensator = compensator(Arc::clone(&store), &failed, Arc::clone(&provider));

        let run = compensator
            .compensate_definite_provision_failure(&failed)
            .await
            .expect("exact failed provision should compensate");

        assert_eq!(run.disposition(), WorkloadTeardownRunDisposition::Completed);
        assert_eq!(run.record().phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(store.record(), *run.record());
        assert_eq!(
            provider
                .calls()
                .iter()
                .map(|call| call.step)
                .collect::<Vec<_>>(),
            expected_steps,
            "failed step {failed_step:?} must release only established effects",
        );
    }
}

#[tokio::test]
async fn lost_cause_response_is_adopted_before_one_teardown_sequence() {
    let failed = failed_provision_record(
        "compensation-ambiguous-cause",
        WorkloadProvisionStep::ObservePublication,
    );
    let store =
        DurableTeardownStore::with_record_and_fault(failed.clone(), CasFault::AmbiguousAfterApply);
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let compensator = compensator(Arc::clone(&store), &failed, Arc::clone(&provider));

    let first = compensator
        .compensate_definite_provision_failure(&failed)
        .await
        .expect("exact readback should adopt the ambiguous cause commit");
    let replay = compensator
        .compensate_definite_provision_failure(&failed)
        .await
        .expect("exact replay should adopt terminal durable truth");

    assert_eq!(first.record(), replay.record());
    assert_eq!(first.record().phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(provider.calls().len(), 5);
}

#[tokio::test]
async fn crossed_failed_provision_readback_changes_no_bytes_and_calls_no_provider() {
    let failed =
        failed_provision_record("compensation-crossed", WorkloadProvisionStep::AttachNetwork);
    let crossed = provision_record(
        "compensation-crossed",
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let store = DurableTeardownStore::with_record(crossed.clone());
    let provider = RecordingTeardownProvider::new(TeardownProviderBehavior::Succeed);
    let compensator = compensator(Arc::clone(&store), &failed, Arc::clone(&provider));

    let error = compensator
        .compensate_definite_provision_failure(&failed)
        .await
        .expect_err("crossed lifecycle evidence must fail closed");

    assert!(matches!(error, WorkloadProvisionCompensationError::Saga(_)));
    assert_eq!(store.record(), crossed);
    assert!(provider.calls().is_empty());
}
