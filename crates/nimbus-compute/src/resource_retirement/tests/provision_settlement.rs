use std::collections::BTreeMap;

use nimbus_workloads::{
    DesiredWorkloadState, WorkloadProvisionCommandMode, WorkloadProvisionStep, WorkloadSagaPhase,
};

use super::support::{
    LifecycleEvent, RetirementHarness, SANDBOX_ID, SERVICE_NAME, assert_teardown_effect_order, key,
    prior_process_issued_provision_record, run_async_test, sandbox_spec,
};
use crate::workload_provisioner::WorkloadProvisionCancellation;

#[test]
fn service_stop_joins_inflight_provision_and_retires_late_success() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        let (entered, release) = harness
            .provision_provider
            .install_gate(WorkloadProvisionStep::ReserveNetwork);
        let provision = harness.provision.clone();
        let context = harness.context.clone();
        let start = tokio::spawn(async move {
            provision
                .provision_sandbox_service(
                    &context,
                    SERVICE_NAME,
                    &WorkloadProvisionCancellation::default(),
                )
                .await
        });
        entered
            .acquire()
            .await
            .expect("provision should enter the exact provider boundary")
            .forget();

        let source_claim = harness.install_source_claim_signal();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let stop =
            tokio::spawn(
                async move { retire.submit_service_teardown(&context, SERVICE_NAME).await },
            );
        harness
            .wait_for_source_claim(&source_claim, "service")
            .await;
        assert!(
            harness.service_source_is_fenced(),
            "stop must install the source fence while joining retained provision"
        );
        release.add_permits(1);

        start
            .await
            .expect("provision task should join")
            .expect("late provision success should remain observable");
        stop.await
            .expect("stop task should join")
            .expect("stop should retire the late success");
        let recorded = harness.store.record(&key(SERVICE_NAME));
        assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            recorded.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_teardown_effect_order(&harness.log.entries(), &key(SERVICE_NAME));
    });
}

#[test]
fn sandbox_stop_joins_inflight_provision_and_retires_late_success() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        let (entered, release) = harness
            .provision_provider
            .install_gate(WorkloadProvisionStep::ReserveNetwork);
        let provision = harness.provision.clone();
        let context = harness.context.clone();
        let start = tokio::spawn(async move {
            provision
                .provision_standalone_sandbox(
                    &context,
                    SANDBOX_ID,
                    "worker",
                    sandbox_spec(),
                    BTreeMap::new(),
                    &WorkloadProvisionCancellation::default(),
                )
                .await
        });
        entered
            .acquire()
            .await
            .expect("provision should enter the exact provider boundary")
            .forget();

        let source_claim = harness.install_source_claim_signal();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let stop =
            tokio::spawn(async move { retire.submit_sandbox_teardown(&context, SANDBOX_ID).await });
        harness
            .wait_for_source_claim(&source_claim, "sandbox")
            .await;
        assert!(
            harness.sandbox_source_is_fenced(),
            "stop must install the source fence while joining retained provision"
        );
        release.add_permits(1);

        start
            .await
            .expect("provision task should join")
            .expect("late provision success should remain observable");
        stop.await
            .expect("stop task should join")
            .expect("stop should retire the late success");
        let recorded = harness.store.record(&key(SANDBOX_ID));
        assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(
            recorded.active_intent().desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_teardown_effect_order(&harness.log.entries(), &key(SANDBOX_ID));
    });
}

#[test]
fn service_stop_fences_start_before_its_first_saga_commit() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        let (cas_entered, release_cas) = harness.store.install_first_missing_cas_gate();
        let provision = harness.provision.clone();
        let context = harness.context.clone();
        let start = tokio::spawn(async move {
            provision
                .provision_sandbox_service(
                    &context,
                    SERVICE_NAME,
                    &WorkloadProvisionCancellation::default(),
                )
                .await
        });
        harness
            .wait_for_signal(
                &cas_entered,
                "service start did not reach its first missing-saga commit",
            )
            .await;
        assert_eq!(harness.provision_provider.call_count(), 0);

        let source_claim = harness.install_source_claim_signal();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let stop =
            tokio::spawn(
                async move { retire.submit_service_teardown(&context, SERVICE_NAME).await },
            );
        harness
            .wait_for_source_claim(&source_claim, "service")
            .await;
        assert!(harness.service_source_is_fenced());
        assert!(!stop.is_finished());
        assert_eq!(harness.provision_provider.call_count(), 0);

        release_cas.add_permits(1);
        start
            .await
            .expect("service start task should join")
            .expect("service start should retain its first saga commit");
        stop.await
            .expect("service stop task should join")
            .expect("service stop should retire the retained start");
        assert_eq!(
            harness
                .store
                .record(&key(SERVICE_NAME))
                .active_intent()
                .desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_teardown_effect_order(&harness.log.entries(), &key(SERVICE_NAME));
    });
}

#[test]
fn sandbox_stop_fences_start_before_its_first_saga_commit() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        let (cas_entered, release_cas) = harness.store.install_first_missing_cas_gate();
        let provision = harness.provision.clone();
        let context = harness.context.clone();
        let start = tokio::spawn(async move {
            provision
                .provision_standalone_sandbox(
                    &context,
                    SANDBOX_ID,
                    "worker",
                    sandbox_spec(),
                    BTreeMap::new(),
                    &WorkloadProvisionCancellation::default(),
                )
                .await
        });
        harness
            .wait_for_signal(
                &cas_entered,
                "sandbox start did not reach its first missing-saga commit",
            )
            .await;
        assert_eq!(harness.provision_provider.call_count(), 0);

        let source_claim = harness.install_source_claim_signal();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let stop =
            tokio::spawn(async move { retire.submit_sandbox_teardown(&context, SANDBOX_ID).await });
        harness
            .wait_for_source_claim(&source_claim, "sandbox")
            .await;
        assert!(harness.sandbox_source_is_fenced());
        assert!(!stop.is_finished());
        assert_eq!(harness.provision_provider.call_count(), 0);

        release_cas.add_permits(1);
        start
            .await
            .expect("sandbox start task should join")
            .expect("sandbox start should retain its first saga commit");
        stop.await
            .expect("sandbox stop task should join")
            .expect("sandbox stop should retire the retained start");
        assert_eq!(
            harness
                .store
                .record(&key(SANDBOX_ID))
                .active_intent()
                .desired_state(),
            DesiredWorkloadState::Stopped
        );
        assert_teardown_effect_order(&harness.log.entries(), &key(SANDBOX_ID));
    });
}

#[test]
fn definition_delete_fences_start_before_its_first_saga_commit() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        let (cas_entered, release_cas) = harness.store.install_first_missing_cas_gate();
        let provision = harness.provision.clone();
        let context = harness.context.clone();
        let start = tokio::spawn(async move {
            provision
                .provision_sandbox_service(
                    &context,
                    SERVICE_NAME,
                    &WorkloadProvisionCancellation::default(),
                )
                .await
        });
        harness
            .wait_for_signal(
                &cas_entered,
                "definition start did not reach its first missing-saga commit",
            )
            .await;

        let source_claim = harness.install_source_claim_signal();
        let retire = harness.retire.clone();
        let context = harness.context.clone();
        let delete = tokio::spawn(async move {
            retire
                .submit_definition_teardown(&context, SERVICE_NAME, 1, false)
                .await
        });
        harness
            .wait_for_source_claim(&source_claim, "definition")
            .await;
        assert!(harness.service_source_is_fenced());
        assert!(!delete.is_finished());
        assert!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
                .is_some(),
            "definition delete must retain desire while the pre-CAS start can still commit"
        );
        assert_eq!(harness.provision_provider.call_count(), 0);

        release_cas.add_permits(1);
        start
            .await
            .expect("definition start task should join")
            .expect("definition start should retain its first saga commit");
        delete
            .await
            .expect("definition delete task should join")
            .expect("definition delete should retire the retained start");
        assert!(
            harness
                .manager
                .service_definition_for_tenant(harness.context.tenant_id(), SERVICE_NAME)
                .is_none(),
            "definition delete may remove desire only after recorded teardown"
        );
        assert_teardown_effect_order(&harness.log.entries(), &key(SERVICE_NAME));
    });
}

#[test]
fn prior_process_issued_provision_is_inspected_once_before_teardown() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let issued =
            prior_process_issued_provision_record(&harness.store.record(&key(SERVICE_NAME)));
        harness.store.replace(issued);
        harness.reset_retirement_evidence();

        harness
            .retire
            .submit_service_teardown(&harness.context, SERVICE_NAME)
            .await
            .unwrap_or_else(|error| {
                panic!(
                    "prior-process provision result should settle before teardown: {error:?}; record={:?}; events={:?}",
                    harness.store.record(&key(SERVICE_NAME)),
                    harness.log.entries(),
                )
            });

        let provision = harness
            .log
            .entries()
            .into_iter()
            .filter_map(|event| match event {
                LifecycleEvent::Provision(event_key, step, mode) => Some((event_key, step, mode)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            provision,
            vec![(
                key(SERVICE_NAME),
                WorkloadProvisionStep::ReserveNetwork,
                WorkloadProvisionCommandMode::Inspect,
            )],
            "retirement must inspect the exact prior-process claim once and must not re-execute it"
        );
        let teardown = harness
            .log
            .entries()
            .into_iter()
            .filter_map(|event| match event {
                LifecycleEvent::Teardown(event_key, step, mode) => Some((event_key, step, mode)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            teardown,
            vec![(
                key(SERVICE_NAME),
                nimbus_workloads::WorkloadTeardownStep::ReleaseNetwork,
                nimbus_workloads::WorkloadTeardownCommandMode::Execute,
            )],
            "the exact reserved-network success must be released once without inventing unprovisioned resources"
        );
    });
}
