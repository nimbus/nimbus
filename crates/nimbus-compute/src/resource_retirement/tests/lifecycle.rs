use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadPublicationIntent, WorkloadSagaPhase,
    WorkloadSagaRecord,
};

use crate::WorkloadTeardownDisposition;

use super::support::{
    RetirementHarness, SANDBOX_ID, SERVICE_NAME, assert_complete_teardown_order, key,
    run_async_test,
};

#[test]
fn service_stop_persists_then_observes_complete_teardown_order() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let running = harness.store.record(&key(SERVICE_NAME));
        harness.reset_retirement_evidence();

        let outcome = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "service retirement should converge: {error:?}; events: {:?}",
                    harness.log.entries()
                )
            });

        assert!(outcome.retired_handle.is_some());
        assert_eq!(outcome.disposition(), WorkloadTeardownDisposition::Recorded);
        assert_eq!(
            outcome.terminal_execution_reference(),
            Some(&running.current_execution_reference()),
            "native retirement must return the exact durable execution after projection cleanup"
        );
        let observation = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("terminal service observation should remain truthful");
        assert_eq!(
            observation.handle.status,
            nimbus_sandbox::SandboxStatus::Stopped
        );
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 1);
        assert!(observation.handle.published_endpoints.is_empty());
        let record = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            record.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_terminal_network_successor(&running, &record);
        assert_complete_teardown_order(&harness.log.entries(), &key(SERVICE_NAME));
    });
}

#[test]
fn unstarted_service_retirement_reports_source_finalized_without_execution_identity() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();

        let outcome = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("an unstarted source should finalize without provider effects");

        assert_eq!(
            outcome.disposition(),
            WorkloadTeardownDisposition::SourceFinalized
        );
        assert!(outcome.terminal_execution_reference().is_none());
        assert!(outcome.retired_handle.is_none());
        assert_eq!(harness.teardown_provider.call_count(), 0);
    });
}

#[test]
fn sandbox_stop_persists_then_observes_complete_teardown_order() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.start_sandbox().await;
        let running = harness.store.record(&key(SANDBOX_ID));
        harness.reset_retirement_evidence();

        let snapshot = harness
            .retire
            .submit_sandbox_teardown(&harness.context, SANDBOX_ID)
            .await
            .expect("sandbox retirement should converge");

        let observation = snapshot
            .observation
            .expect("terminal sandbox observation should remain truthful");
        assert_eq!(
            observation.handle.status,
            nimbus_sandbox::SandboxStatus::Stopped
        );
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 1);
        assert!(observation.handle.published_endpoints.is_empty());
        let record = harness.store.record(&key(SANDBOX_ID));
        assert_eq!(record.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            record.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_terminal_network_successor(&running, &record);
        assert_complete_teardown_order(&harness.log.entries(), &key(SANDBOX_ID));
    });
}

fn assert_terminal_network_successor(running: &WorkloadSagaRecord, stopped: &WorkloadSagaRecord) {
    let running = running.active_intent().network().compiled_plan().content();
    let stopped = stopped.active_intent().network().compiled_plan().content();

    assert_eq!(
        stopped.identity().workload_incarnation_key(),
        running.identity().workload_incarnation_key(),
        "retirement must retain the admitted workload-incarnation identity"
    );
    assert_eq!(
        stopped.identity().plan_id(),
        running.identity().plan_id(),
        "retirement must not create a second stable network-plan identity"
    );
    assert_eq!(
        stopped.sovereignty_requirements(),
        running.sovereignty_requirements(),
        "a terminal empty plan must retain the complete admitted sovereignty baseline"
    );
    assert!(stopped.identity().generation() > running.identity().generation());
    assert!(stopped.attachment().is_none());
    assert!(stopped.routes().is_empty());
    assert!(stopped.listeners().is_empty());
    assert!(stopped.dependency_listeners().is_empty());
    assert_eq!(stopped.activation(), WorkloadActivationIntent::PrepareOnly);
    assert_eq!(stopped.publication(), WorkloadPublicationIntent::Withheld);
}
