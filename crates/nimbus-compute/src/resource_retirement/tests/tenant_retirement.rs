use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use nimbus_workloads::{
    DesiredWorkloadState, WorkloadSagaPhase, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};

use crate::tenant_retirement::{TenantRetirementDriver, TenantRetirementError};
use crate::workload_saga::WorkloadSagaCoordinator;

use super::support::{
    LifecycleEvent, RetirementHarness, SANDBOX_ID, SERVICE_NAME, TenantPageFault,
    issued_restart_record, key, run_async_test, tenant,
};

#[test]
fn tenant_driver_retires_every_durable_child_before_effect_free_finalization() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture both workloads");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let coordinator = Arc::new(WorkloadSagaCoordinator::new(store));
        let driver = TenantRetirementDriver::new(
            coordinator,
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("every durable child should converge before finalization");

        assert_eq!(terminal.len(), 2);
        assert!(terminal.iter().all(|record| {
            record.phase() == WorkloadSagaPhase::Recorded
                && record.active_intent().desired_state() == DesiredWorkloadState::Stopped
                && record.successor_intent().is_none()
        }));
        assert_eq!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Recorded
        );
        assert_eq!(
            harness.store.record(&key(SANDBOX_ID)).phase(),
            WorkloadSagaPhase::Recorded
        );
        assert!(
            harness
                .manager
                .create_service_definition(
                    &tenant(),
                    "blocked-before-engine-finish",
                    nimbus_services::ServiceBackend::sandbox(super::support::service_spec()),
                    std::collections::BTreeMap::new(),
                )
                .is_err(),
            "effect-free finalization must retain the tenant barrier until Engine finish"
        );
        harness
            .manager
            .release_tenant_source_retirement(snapshot.claim())
            .expect("post-Engine release should accept the exact finalized claim");
    });
}

#[test]
fn tenant_driver_paginates_and_drives_each_exact_key_once_per_pass() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture both workloads");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        )
        .with_page_size_for_test(1);

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("both bounded pages should converge");

        assert_eq!(terminal.len(), 2);
        assert_eq!(
            harness.store.tenant_page_call_count(),
            4,
            "both the initial and final inventories must consume two bounded pages"
        );
        let mut effects_per_key = BTreeMap::new();
        for event in harness.log.entries() {
            if let LifecycleEvent::Teardown(event_key, _, _) = event {
                *effects_per_key.entry(event_key).or_insert(0) += 1;
            }
        }
        assert_eq!(effects_per_key.get(&key(SERVICE_NAME)), Some(&5));
        assert_eq!(effects_per_key.get(&key(SANDBOX_ID)), Some(&5));
        assert_eq!(effects_per_key.len(), 2);
    });
}

#[test]
fn tenant_driver_joins_pre_first_cas_provision_before_inventory_and_finalization() {
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
                    &crate::workload_provisioner::WorkloadProvisionCancellation::default(),
                )
                .await
        });
        harness
            .wait_for_signal(
                &cas_entered,
                "service provision did not reach its first durable saga CAS",
            )
            .await;
        assert_eq!(harness.provision_provider.call_count(), 0);

        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant barrier should capture the reserved source");
        let source_claim = harness.install_source_claim_signal();
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );
        let retirement = tokio::spawn(async move { driver.drive_tenant_teardown(&snapshot).await });
        harness
            .wait_for_signal(
                &source_claim,
                "tenant retirement did not install its batch source-key fence",
            )
            .await;
        assert!(
            !retirement.is_finished(),
            "tenant retirement must join tracked work that has not made its first saga CAS"
        );
        assert_eq!(harness.store.tenant_page_call_count(), 0);
        assert_eq!(harness.provision_provider.call_count(), 0);

        release_cas.add_permits(1);
        start
            .await
            .expect("provision task should join")
            .expect("the retained provision should finish before retirement");
        let terminal = retirement
            .await
            .expect("tenant retirement task should join")
            .expect("the retained provision should be inventoried and retired");

        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].key(), &key(SERVICE_NAME));
        assert_eq!(terminal[0].phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(harness.teardown_provider.call_count(), 5);
    });
}

#[test]
fn tenant_driver_rejects_concurrent_child_insertion_without_duplicate_effects() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture both workloads");
        harness.store.fault_tenant_page(
            2,
            TenantPageFault::Insert(Box::new(orphan_record_for_retirement_tenant())),
        );
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        assert!(matches!(
            driver.drive_tenant_teardown(&snapshot).await,
            Err(TenantRetirementError::InvalidInventory(
                "durable workload key set changed during tenant retirement"
            ))
        ));
        assert_eq!(harness.teardown_provider.call_count(), 10);
        assert!(
            harness
                .manager
                .create_service_definition(
                    &tenant(),
                    "blocked-after-concurrent-insert",
                    nimbus_services::ServiceBackend::sandbox(super::support::service_spec()),
                    BTreeMap::new(),
                )
                .is_err(),
            "an unsuccessful deletion must retain the tenant source barrier"
        );

        assert!(matches!(
            driver.drive_tenant_teardown(&snapshot).await,
            Err(TenantRetirementError::InvalidInventory(
                "durable record does not belong to the frozen source snapshot"
            ))
        ));
        assert_eq!(
            harness.teardown_provider.call_count(),
            10,
            "retry must not repeat already recorded sibling effects"
        );
    });
}

#[test]
fn tenant_driver_retries_after_one_sibling_store_failure_without_duplicate_effects() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture both workloads");
        harness.store.fail_next_load_for(key(SANDBOX_ID));
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        assert!(matches!(
            driver.drive_tenant_teardown(&snapshot).await,
            Err(TenantRetirementError::Teardown(_))
        ));
        assert_eq!(
            harness.store.record(&key(SERVICE_NAME)).phase(),
            WorkloadSagaPhase::Recorded
        );
        assert_eq!(
            harness.store.record(&key(SANDBOX_ID)).phase(),
            WorkloadSagaPhase::Observed
        );
        assert_eq!(harness.teardown_provider.call_count(), 5);
        assert!(
            harness
                .manager
                .create_service_definition(
                    &tenant(),
                    "blocked-after-sibling-failure",
                    nimbus_services::ServiceBackend::built_in("browser"),
                    BTreeMap::new(),
                )
                .is_err(),
            "an unsuccessful sibling pass must retain the tenant admission barrier"
        );

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("retry should preserve the first sibling and finish the second");
        assert_eq!(terminal.len(), 2);
        assert_eq!(
            harness.teardown_provider.call_count(),
            10,
            "each sibling must run one exact five-effect sequence across retry"
        );
    });
}

#[test]
fn tenant_driver_settles_an_issued_restart_before_teardown() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let issued = issued_restart_record(&harness.store.record(&key(SERVICE_NAME)));
        harness.store.replace(issued);
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture the service");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("issued restart must settle before tenant teardown");

        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].phase(), WorkloadSagaPhase::Recorded);
        assert_eq!(harness.restart_provider.execute_call_count(), 0);
        assert_eq!(harness.restart_provider.inspect_call_count(), 1);
        let events = harness.log.entries();
        let inspection = events
            .iter()
            .position(|event| {
                matches!(
                    event,
                    LifecycleEvent::Restart(
                        event_key,
                        _,
                        crate::workload_saga::WorkloadRestartCommandMode::Inspect
                    ) if event_key == &key(SERVICE_NAME)
                )
            })
            .expect("issued restart must be inspected");
        let withdrawal = events
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
            .expect("withdrawal must become durable");
        assert!(inspection < withdrawal);
        assert_eq!(harness.teardown_provider.call_count(), 5);
    });
}

#[test]
fn tenant_driver_isolates_other_tenant_and_rejects_orphan_and_store_faults() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        let other = crate::workload_saga::test_support::observed_fixture_record("other");
        harness.store.replace(other.clone());
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("target tenant should claim");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("other-tenant durable truth must not enter this retirement");
        assert_eq!(harness.store.record(other.key()), other);
        assert!(harness.log.entries().iter().all(|event| !matches!(
            event,
            LifecycleEvent::Teardown(event_key, _, _) if event_key.tenant_id() != &tenant()
        )));

        let orphan_harness = RetirementHarness::new();
        orphan_harness
            .store
            .replace(orphan_record_for_retirement_tenant());
        let orphan_snapshot = orphan_harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("empty source inventory should claim");
        let orphan_store: Arc<dyn WorkloadSagaStore> = orphan_harness.store.clone();
        let orphan_driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(orphan_store)),
            Arc::clone(&orphan_harness.manager),
            orphan_harness.retire.clone(),
        );
        assert!(matches!(
            orphan_driver.drive_tenant_teardown(&orphan_snapshot).await,
            Err(TenantRetirementError::InvalidInventory(
                "durable record does not belong to the frozen source snapshot"
            ))
        ));
        assert_eq!(orphan_harness.teardown_provider.call_count(), 0);

        for error in [
            WorkloadSagaStoreError::Corrupt,
            WorkloadSagaStoreError::Unavailable,
        ] {
            let fault_harness = RetirementHarness::new();
            let fault_snapshot = fault_harness
                .manager
                .claim_tenant_source_retirement(
                    &tenant(),
                    NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
                )
                .expect("empty source inventory should claim");
            fault_harness
                .store
                .fault_tenant_page(1, TenantPageFault::Error(error.clone()));
            let fault_store: Arc<dyn WorkloadSagaStore> = fault_harness.store.clone();
            let fault_driver = TenantRetirementDriver::new(
                Arc::new(WorkloadSagaCoordinator::new(fault_store)),
                Arc::clone(&fault_harness.manager),
                fault_harness.retire.clone(),
            );
            assert!(matches!(
                fault_driver.drive_tenant_teardown(&fault_snapshot).await,
                Err(TenantRetirementError::Inventory(observed)) if observed == error
            ));
            assert_eq!(fault_harness.teardown_provider.call_count(), 0);
            assert_eq!(
                fault_harness
                    .manager
                    .claim_tenant_source_retirement(
                        &tenant(),
                        NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
                    )
                    .expect("failed inventory must retain the exact source barrier"),
                fault_snapshot
            );
        }
    });
}

#[test]
fn tenant_driver_rejects_pages_not_bound_to_the_issued_tenant_cursor() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        harness.reset_retirement_evidence();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("tenant source barrier should capture both workloads");
        let stale_request = WorkloadSagaTenantPageRequest::new(None, 1)
            .expect("stale fixture request should validate");
        let stale_page = WorkloadSagaTenantPage::new(
            &tenant(),
            &stale_request,
            vec![harness.store.record(&key(SERVICE_NAME))],
            false,
        )
        .expect("stale page is valid only against its own request");
        harness
            .store
            .fault_tenant_page(2, TenantPageFault::Page(stale_page));
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        )
        .with_page_size_for_test(1);
        assert!(matches!(
            driver.drive_tenant_teardown(&snapshot).await,
            Err(TenantRetirementError::Inventory(
                WorkloadSagaStoreError::Corrupt
            ))
        ));
        assert_eq!(
            harness.teardown_provider.call_count(),
            0,
            "a stale or cursor-regressing inventory must fail before effects"
        );

        let crossed_harness = RetirementHarness::new();
        let crossed_snapshot = crossed_harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("empty source inventory should claim");
        let other_tenant = nimbus_core::TenantId::new("tenant-page-crossed")
            .expect("crossed tenant should validate");
        let request = WorkloadSagaTenantPageRequest::new(None, 1)
            .expect("crossed page request should validate");
        let crossed_page = WorkloadSagaTenantPage::new(&other_tenant, &request, Vec::new(), false)
            .expect("empty crossed page should validate for its declared tenant");
        crossed_harness
            .store
            .fault_tenant_page(1, TenantPageFault::Page(crossed_page));
        let crossed_store: Arc<dyn WorkloadSagaStore> = crossed_harness.store.clone();
        let crossed_driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(crossed_store)),
            Arc::clone(&crossed_harness.manager),
            crossed_harness.retire.clone(),
        );
        assert!(matches!(
            crossed_driver
                .drive_tenant_teardown(&crossed_snapshot)
                .await,
            Err(TenantRetirementError::Inventory(
                WorkloadSagaStoreError::Corrupt
            ))
        ));
        assert_eq!(crossed_harness.teardown_provider.call_count(), 0);
    });
}

#[test]
fn unstarted_source_requires_no_fabricated_saga_or_provider_effect() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(
                &tenant(),
                NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
            )
            .expect("unstarted source should be captured without a saga");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("unstarted source should finalize without invented effects");

        assert!(terminal.is_empty());
        assert_eq!(harness.teardown_provider.call_count(), 0);
    });
}

#[test]
fn tenant_delete_waits_for_every_durable_workload_teardown_before_storage_delete() {
    run_async_test(async {
        let harness = RetirementHarness::new();
        harness.declare_service();
        harness.start_service().await;
        harness.start_sandbox().await;
        let engine_root = tempfile::tempdir().expect("fixture Engine root should build");
        let engine = Arc::new(
            nimbus_engine::Engine::new(engine_root.path()).expect("fixture Engine should build"),
        );
        engine
            .create_tenant_async(tenant())
            .await
            .expect("tenant storage should exist before retirement");
        let deletion = engine
            .begin_tenant_delete_async(tenant())
            .await
            .expect("Engine should install its deletion fence");
        let retired_incarnation = deletion.tenant_incarnation();
        let snapshot = harness
            .manager
            .claim_tenant_source_retirement(&tenant(), deletion.tenant_incarnation())
            .expect("services should install the same-incarnation source barrier");
        let store: Arc<dyn WorkloadSagaStore> = harness.store.clone();
        let driver = TenantRetirementDriver::new(
            Arc::new(WorkloadSagaCoordinator::new(store)),
            Arc::clone(&harness.manager),
            harness.retire.clone(),
        );

        let terminal = driver
            .drive_tenant_teardown(&snapshot)
            .await
            .expect("all durable children should stop before storage deletion");

        assert_eq!(terminal.len(), 2);
        assert!(
            engine
                .list_tenants_async()
                .await
                .expect("tenant registry should list while deletion is fenced")
                .contains(&tenant()),
            "Engine persistence must still exist after child teardown and services finalization"
        );
        engine
            .finish_tenant_delete_async(deletion)
            .await
            .expect("Engine storage deletion should follow terminal child truth");
        assert!(
            !engine
                .list_tenants_async()
                .await
                .expect("tenant registry should list after deletion")
                .contains(&tenant()),
            "Engine storage must be absent only after every durable child is terminal"
        );
        harness
            .manager
            .release_tenant_source_retirement(snapshot.claim())
            .expect("services barrier should release only after Engine finish");
        harness
            .manager
            .create_service_definition(
                &tenant(),
                SERVICE_NAME,
                nimbus_services::ServiceBackend::sandbox(super::support::service_spec()),
                BTreeMap::new(),
            )
            .expect("source admission should reopen after exact barrier release");
        engine
            .create_tenant_async(tenant())
            .await
            .expect("the same tenant ID should recreate after deletion");
        let recreated = engine
            .begin_tenant_delete_async(tenant())
            .await
            .expect("recreated tenant should have a new deletion identity");
        assert!(recreated.tenant_incarnation() > retired_incarnation);
        engine
            .finish_tenant_delete_async(recreated)
            .await
            .expect("recreated fixture tenant should clean up");
    });
}

fn orphan_record_for_retirement_tenant() -> nimbus_workloads::WorkloadSagaRecord {
    crate::workload_saga::test_support::observed_fixture_record("retirement")
}
