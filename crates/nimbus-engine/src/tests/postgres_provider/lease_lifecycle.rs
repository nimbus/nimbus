use nimbus_core::{
    PrincipalContext, SequenceNumber, TenantEventKind, TenantEventRecord, TriggerDeliveryCursor,
};
use nimbus_storage::{ManualClock, NoopFaultInjector};

use super::support::*;
use crate::commit_fault_labels as labels;

async fn provider_engine(config: EnginePersistenceConfig, clock: Arc<ManualClock>) -> Arc<Engine> {
    Arc::new(
        Engine::new_with_simulation_and_persistence_config(
            config,
            clock,
            Arc::new(NoopFaultInjector),
        )
        .await
        .expect("postgres-backed engine should create"),
    )
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_lease_is_lazy_idempotent_renewed_and_cancelled_with_the_runtime() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
        let engine = provider_engine(engine_config, clock.clone()).await;
        let tenant_id = TenantId::new("pg-lazy-lease").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let provider = PostgresProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");

        let loaded = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("loaded stats should read");
        assert!(!loaded.committer_lease_acquired);
        assert_eq!(loaded.committer_lease_acquire_count, 0);
        assert!(
            opened
                .store
                .read_committer_lease()
                .expect("lease read should succeed")
                .is_none(),
            "tenant construction must not acquire sequence authority"
        );

        let first_writer = engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        );
        let concurrent_first_writer = engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("concurrent"))]),
        );
        let (first_result, concurrent_result) = tokio::join!(first_writer, concurrent_first_writer);
        first_result.expect("first assignment should acquire and commit");
        concurrent_result.expect("concurrent first writer should reuse the acquired lease");
        let first = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("first-assignment stats should read");
        assert!(first.committer_lease_acquired);
        assert_eq!(first.committer_lease_epoch, 1);
        assert_eq!(first.committer_lease_acquire_count, 1);
        assert!(first.committer_lease_renewal_worker_running);
        let initial_expiry = first.committer_lease_expires_at;

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("later"))]),
            )
            .await
            .expect("later assignment should reuse the lease");
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("second-assignment stats should read")
                .committer_lease_acquire_count,
            1,
            "one runtime must acquire at most once"
        );

        clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("renewal worker should wake");
        let renewed = wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "manual-clock renewal should complete",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;
        assert!(renewed.committer_lease_expires_at > initial_expiry);
        assert_eq!(renewed.committer_lease_renewal_failure_count, 0);

        engine.quiesce().await;
        assert!(
            !engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("quiesced stats should read")
                .committer_lease_renewal_worker_running,
            "engine quiesce must join the tenant lease worker"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn two_postgres_engines_load_without_leases_and_only_one_first_writer_acquires_epoch() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualClock::new(Timestamp(20_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualClock::new(Timestamp(20_000)))).await;
        let tenant_id = TenantId::new("pg-two-engine-lease").expect("tenant id should build");
        engine_a
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("second engine should load the same tenant");

        for engine in [&engine_a, &engine_b] {
            let stats = engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("loaded stats should read");
            assert!(!stats.committer_lease_acquired);
            assert_eq!(stats.committer_lease_acquire_count, 0);
        }
        let provider = PostgresProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        assert!(
            opened
                .store
                .read_committer_lease()
                .expect("lease read should succeed")
                .is_none()
        );

        let write_a = engine_a.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("a"))]),
        );
        let write_b = engine_b.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("b"))]),
        );
        let (result_a, result_b) = tokio::join!(write_a, write_b);
        assert_eq!(
            usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
            1
        );

        let stats_a = engine_a
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("first engine stats should read");
        let stats_b = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("second engine stats should read");
        assert_eq!(
            usize::from(stats_a.committer_lease_acquired)
                + usize::from(stats_b.committer_lease_acquired),
            1
        );
        assert_ne!(
            (stats_a.committer_lease_epoch, stats_b.committer_lease_epoch),
            (1, 1),
            "two runtimes must never both believe they acquired epoch 1"
        );
        let lease = opened
            .store
            .read_committer_lease()
            .expect("lease read should succeed")
            .expect("one first writer should create the lease");
        assert_eq!(lease.epoch, 1);

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_acquisition_reconciles_predecessor_heads_and_records_fenced_renewal() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let clock_a = Arc::new(ManualClock::new(Timestamp(30_000)));
        let clock_b = Arc::new(ManualClock::new(Timestamp(30_000)));
        let engine_a = provider_engine(config_a, clock_a).await;
        let engine_b = provider_engine(config_b, clock_b.clone()).await;
        let tenant_id = TenantId::new("pg-reconcile-lease").expect("tenant id should build");
        engine_a
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("second engine should load the empty tenant");

        let unit = engine_b
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should begin");
        unit.insert_document(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("successor"))]),
        )
        .expect("successor insert should stage");
        let faults = engine_b.commit_fault_handle_for_testing();
        faults.arm(labels::PRE_ASSIGN);
        let commit = tokio::task::spawn_blocking({
            let unit = unit.clone();
            move || unit.commit()
        });
        let paused = tokio::task::spawn_blocking({
            let faults = faults.clone();
            move || faults.wait_until_entered(labels::PRE_ASSIGN, Duration::from_secs(5))
        })
        .await
        .expect("fault wait should join");
        assert!(paused, "successor must pause before lease acquisition");

        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("hint listeners should terminate before staging predecessor progress");
        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        let predecessor = TenantEventRecord::from_events(
            SequenceNumber(1),
            Timestamp(30_001),
            vec![TenantEventKind::TriggerDelivery {
                cursor: TriggerDeliveryCursor::new(SequenceNumber(0)),
            }],
        )
        .expect("predecessor record should build");
        opened
            .store
            .append_durable_records_batch(std::slice::from_ref(&predecessor))
            .expect("predecessor should append");
        opened
            .store
            .apply_durable_records_batch(std::slice::from_ref(&predecessor))
            .expect("predecessor should apply");
        assert_eq!(
            engine_b
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("paused runtime stats should read")
                .durable_head,
            SequenceNumber(0),
            "the loaded runtime must still need acquisition reconciliation"
        );

        faults.release(labels::PRE_ASSIGN);
        let successor = commit
            .await
            .expect("successor task should join")
            .expect("successor should commit")
            .expect("successor should append a record");
        assert_eq!(successor.sequence, SequenceNumber(2));
        let reconciled = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("reconciled stats should read");
        assert_eq!(reconciled.durable_head, SequenceNumber(2));
        assert_eq!(reconciled.applied_head, SequenceNumber(2));
        assert_eq!(reconciled.committer_lease_epoch, 1);

        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("first lease should expire deterministically");
        engine_a
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("new-owner"))]),
            )
            .await
            .expect("second runtime should acquire the next epoch");
        assert_eq!(
            engine_a
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("new owner stats should read")
                .committer_lease_epoch,
            2
        );

        clock_b.advance(Duration::from_secs(10));
        engine_b
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("old owner renewal should wake");
        let fenced = wait_for_mutation_journal_stats(
            &engine_b,
            &tenant_id,
            "old owner renewal should record fencing",
            |stats| stats.committer_lease_fenced,
        )
        .await;
        assert_eq!(fenced.committer_lease_epoch, 1);
        assert_eq!(fenced.committer_lease_renewal_failure_count, 1);
        assert!(!fenced.committer_lease_renewal_worker_running);
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("unit 3 records fencing without evicting the runtime");

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}
