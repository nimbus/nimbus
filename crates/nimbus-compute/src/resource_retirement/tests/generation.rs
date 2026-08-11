use nimbus_workloads::{DesiredWorkloadState, WorkloadGeneration, WorkloadSagaPhase};

use super::support::{RetirementHarness, SANDBOX_ID, SERVICE_NAME, key, run_async_test};

#[test]
fn service_start_after_recorded_stop_uses_next_lifecycle_generation() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("service stop should record");
        let stopped = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(stopped.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            stopped.active_intent().generation(),
            WorkloadGeneration::new(2)
        );

        harness.start_service().await;

        let restarted = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(restarted.phase(), WorkloadSagaPhase::Observed);
        assert_eq!(
            restarted.active_intent().desired_state(),
            DesiredWorkloadState::Running
        );
        assert_eq!(
            restarted.active_intent().generation(),
            WorkloadGeneration::new(3)
        );
        let observation = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("later service execution should project");
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 3);
    });
}

#[test]
fn sandbox_start_after_recorded_stop_uses_next_lifecycle_generation() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.start_sandbox().await;
        harness
            .retire
            .submit_sandbox_teardown(&harness.context, SANDBOX_ID)
            .await
            .expect("sandbox stop should record");
        let stopped = harness.store.record(&key(SANDBOX_ID));
        assert_eq!(stopped.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            stopped.active_intent().generation(),
            WorkloadGeneration::new(2)
        );

        harness.start_sandbox().await;

        let restarted = harness.store.record(&key(SANDBOX_ID));
        assert_eq!(restarted.phase(), WorkloadSagaPhase::Observed);
        assert_eq!(
            restarted.active_intent().desired_state(),
            DesiredWorkloadState::Running
        );
        assert_eq!(
            restarted.active_intent().generation(),
            WorkloadGeneration::new(3)
        );
        let observation = harness
            .manager
            .sandbox_resource_snapshot_for_tenant(harness.context.tenant_id(), SANDBOX_ID)
            .expect("later sandbox snapshot should resolve")
            .expect("later sandbox source should remain")
            .observation
            .expect("later sandbox execution should project");
        assert_eq!(observation.source_generation, 1);
        assert_eq!(observation.observed_execution_generation, 3);
    });
}

#[test]
fn source_generation_remains_stable_across_stop_and_later_start() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let initial = harness.store.record(&key(SERVICE_NAME));
        let initial_source_generation = initial.active_intent().source().source_generation();

        harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("service stop should record");
        let stopped = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(
            stopped.active_intent().source().source_generation(),
            initial_source_generation
        );

        harness.start_service().await;
        let restarted = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(
            restarted.active_intent().source().source_generation(),
            initial_source_generation,
            "services-owned source generation must not become workload lifecycle identity"
        );
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
                .expect("definition should remain")
                .generation,
            initial_source_generation.as_u64()
        );
    });
}
