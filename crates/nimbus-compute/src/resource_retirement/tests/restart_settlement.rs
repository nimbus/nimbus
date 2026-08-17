use nimbus_workloads::{WorkloadRestartDisposition, WorkloadRestartStep, WorkloadSagaPhase};

use super::support::{
    LifecycleEvent, RetirementHarness, SERVICE_NAME, active_restart_record,
    assert_teardown_effect_order, issued_restart_record, key, run_async_test,
};
use crate::workload_saga::WorkloadRestartCommandMode;

#[test]
fn active_restart_settles_before_withdrawal_committed() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let observed = harness.store.record(&key(SERVICE_NAME));
        let unissued = active_restart_record(&observed);
        assert!(
            unissued
                .restart_state()
                .active()
                .and_then(|active| active.disposition().claim())
                .is_none(),
            "the former fixture had no issued provider claim"
        );
        assert_eq!(harness.restart_provider.execute_call_count(), 0);
        assert_eq!(harness.restart_provider.inspect_call_count(), 0);

        let issued = issued_restart_record(&observed);
        assert_eq!(issued.phase(), WorkloadSagaPhase::Observed);
        assert!(matches!(
            issued
                .restart_state()
                .active()
                .expect("issued restart should remain active")
                .disposition(),
            WorkloadRestartDisposition::DispatchPending { claim }
                if claim.step() == WorkloadRestartStep::QuiesceExecution
        ));
        harness.store.replace(issued);
        let definition_before = harness
            .manager
            .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("service definition should remain present");
        let observation_before = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("running service observation should remain present");
        harness.reset_retirement_evidence();

        let outcome = harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .expect("exact restart settlement should converge through terminal teardown");
        assert_eq!(
            outcome.disposition(),
            crate::WorkloadTeardownDisposition::Recorded
        );

        let events = harness.log.entries();
        let restart_inspections = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                LifecycleEvent::Restart(event_key, step, mode)
                    if event_key == &key(SERVICE_NAME)
                        && *step == WorkloadRestartStep::QuiesceExecution
                        && *mode == WorkloadRestartCommandMode::Inspect =>
                {
                    Some(index)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            restart_inspections.len(),
            1,
            "teardown must inspect the exact issued restart once"
        );
        let withdrawal_index = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    LifecycleEvent::Store {
                        phase: WorkloadSagaPhase::WithdrawalCommitted,
                        ..
                    }
                )
            })
            .expect("retirement should durably enter withdrawal");
        assert!(
            restart_inspections[0] < withdrawal_index,
            "exact restart inspection and its result must settle before WithdrawalCommitted"
        );
        assert_eq!(
            harness.restart_provider.execute_call_count(),
            0,
            "recovery must not execute an already-issued restart again"
        );
        assert_eq!(harness.restart_provider.inspect_call_count(), 1);
        assert!(events.iter().all(|event| !matches!(
            event,
            LifecycleEvent::Restart(_, _, WorkloadRestartCommandMode::Execute)
        )));
        let recorded = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
        assert!(recorded.restart_state().active().is_none());
        assert!(recorded.teardown_disposition().is_none());
        assert_eq!(
            recorded.active_intent().desired_state(),
            nimbus_workloads::DesiredWorkloadState::Stopped
        );
        assert!(recorded.successor_intent().is_none());
        assert_teardown_effect_order(&events, &key(SERVICE_NAME));
        let recorded_index = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    LifecycleEvent::Store {
                        phase: WorkloadSagaPhase::Recorded,
                        ..
                    }
                )
            })
            .expect("restart settlement teardown must persist Recorded");
        assert!(withdrawal_index < recorded_index);
        assert_eq!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME),
            Some(definition_before),
            "service stop must retain its desired definition"
        );
        let observation_after = harness
            .manager
            .service_definition_observation_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
            .expect("recorded restart settlement must publish terminal projection");
        assert_ne!(observation_after, observation_before);
        assert_eq!(
            observation_after.handle.status,
            nimbus_sandbox::SandboxStatus::Stopped
        );
        assert!(observation_after.handle.published_endpoints.is_empty());
    });
}
