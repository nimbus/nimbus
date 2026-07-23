use nimbus_core::{
    ManualWallClock, PrincipalContext, SequenceNumber, TenantEventKind, TenantEventRecord,
    TriggerDeliveryCursor,
};
use nimbus_storage::provider_test_fixtures::PostgresLeaseTimeControl;
use nimbus_storage::{FaultInjector, FaultPoint, NoopFaultInjector};

use super::support::*;
use crate::commit_fault_labels as labels;
use crate::engine::DurableWriteRoute;
use crate::tenant::ManualLeaseRenewalClock;

async fn provider_engine(
    config: EnginePersistenceConfig,
    clock: Arc<ManualWallClock>,
) -> Arc<Engine> {
    provider_engine_with_faults(config, clock, Arc::new(NoopFaultInjector)).await
}

async fn provider_engine_with_faults(
    config: EnginePersistenceConfig,
    clock: Arc<ManualWallClock>,
    faults: Arc<dyn FaultInjector>,
) -> Arc<Engine> {
    Arc::new(
        Engine::new_with_simulation_and_persistence_config(config, clock, faults)
            .await
            .expect("postgres-backed engine should create"),
    )
}

async fn provider_engine_with_lease_clock(
    config: EnginePersistenceConfig,
    clock: Arc<ManualWallClock>,
    lease_clock: Arc<ManualLeaseRenewalClock>,
) -> Arc<Engine> {
    Arc::new(
        Engine::new_with_simulation_and_persistence_config_and_lease_clock(
            config,
            clock,
            Arc::new(NoopFaultInjector),
            lease_clock,
        )
        .await
        .expect("postgres-backed engine with lease clock should create"),
    )
}

#[derive(Default)]
struct ArmedProviderCommitAcknowledgementLoss {
    armed: std::sync::atomic::AtomicBool,
    fired: std::sync::atomic::AtomicBool,
}

impl ArmedProviderCommitAcknowledgementLoss {
    fn arm(&self) {
        self.armed.store(true, std::sync::atomic::Ordering::Release);
    }
}

impl FaultInjector for ArmedProviderCommitAcknowledgementLoss {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::StorageCommitAfterVisibilityBeforeReturn
            && self.armed.load(std::sync::atomic::Ordering::Acquire)
            && !self.fired.swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Err(nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected provider publisher acknowledgement loss",
            ));
        }
        Ok(())
    }
}

async fn create_shared_tenant(
    engine_a: &Arc<Engine>,
    engine_b: &Arc<Engine>,
    name: &str,
) -> TenantId {
    let tenant_id = TenantId::new(name).expect("tenant id should build");
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("first engine should create tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("second engine should load tenant without acquiring");
    tenant_id
}

async fn inspection_store(
    provider_config: &PostgresProviderConfig,
    tenant_id: &TenantId,
) -> Arc<nimbus_storage::PostgresTenantStore> {
    PostgresProvider::connect(provider_config.clone())
        .await
        .expect("inspection provider should connect")
        .open_existing_opened_tenant(tenant_id)
        .await
        .expect("tenant lookup should succeed")
        .expect("tenant should exist")
        .store
}

fn assert_terminal_fenced(error: nimbus_core::Error) {
    assert!(
        matches!(error, nimbus_core::Error::CommitterFenced { epoch: 1, .. }),
        "fence loss must retain its typed owner/epoch identity: {error}"
    );
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert_eq!(error.conflicting_sequence(), None);
    assert_ne!(
        error.commit_class(),
        Some(nimbus_core::CommitErrorClass::Conflict),
        "a lease fence is not an OCC conflict"
    );
}

fn title(value: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("title".to_string(), json!(value))])
}

#[path = "lease_lifecycle/internal_durable_jobs.rs"]
mod internal_durable_jobs;
#[path = "lease_lifecycle/ordered_arm.rs"]
mod ordered_arm;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_provider_publisher_ack_loss_is_classified_before_retry_fence() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualWallClock::new(Timestamp(9_500)));
        let faults = Arc::new(ArmedProviderCommitAcknowledgementLoss::default());
        let engine = provider_engine_with_faults(engine_config, clock, faults.clone()).await;
        let tenant_id =
            TenantId::new("pg-publisher-ack-loss").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("trigger cursor should not add unrelated records");
        engine
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "seed")
            .expect("seed publisher barrier should acquire the lease and persist");

        faults.arm();
        let acknowledgement_loss = engine
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "landed-without-ack")
            .expect_err("a lost provider acknowledgement must be terminally ambiguous");
        assert_eq!(
            acknowledgement_loss.retryability(),
            nimbus_core::Retryability::Terminal
        );
        assert!(
            acknowledgement_loss.to_string().contains("crash-and-replay"),
            "provider acknowledgement loss must preserve the first attempt's ambiguity: {acknowledgement_loss}"
        );
        assert_eq!(
            engine.durable_outcome_probe_count_for_testing(
                &tenant_id,
                DurableWriteRoute::Publisher,
            ),
            1,
            "the failed provider attempt must be probed before retry is considered"
        );

        let store = inspection_store(&provider_config, &tenant_id).await;
        let progress = store
            .journal_progress()
            .expect("provider progress should remain readable");
        assert_eq!(progress.durable_head, SequenceNumber(2));
        assert_eq!(progress.applied_head, SequenceNumber(2));
        assert_eq!(
            store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("provider journal should read")
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            vec![SequenceNumber(1), SequenceNumber(2)],
            "the acknowledgement-loss attempt must land exactly once"
        );

        engine
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "next-independent-batch")
            .expect("the progress probe should reconcile the runtime head for later work");
        assert_eq!(
            engine.durable_outcome_probe_count_for_testing(
                &tenant_id,
                DurableWriteRoute::Publisher,
            ),
            1,
            "later successful work must not add another failure-classification probe"
        );
        assert_eq!(
            store
                .read_durable_journal_from(SequenceNumber(1))
                .expect("provider journal should reread")
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            vec![SequenceNumber(1), SequenceNumber(2), SequenceNumber(3)],
            "the next independent batch must continue after, rather than duplicate, the ambiguous sequence"
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_lease_is_lazy_idempotent_renewed_and_cancelled_with_the_runtime() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualWallClock::new(Timestamp(10_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine =
            provider_engine_with_lease_clock(engine_config, clock, lease_clock.clone()).await;
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

        lease_clock.advance(Duration::from_secs(10));
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
async fn lease_renewal_ignores_backward_wall_clock_step() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(100_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = provider_engine_with_lease_clock(
            engine_config,
            wall_clock.clone(),
            lease_clock.clone(),
        )
        .await;
        let tenant_id = TenantId::new("pg-lease-backward-clock").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("acquire"))
            .await
            .expect("first write should acquire the lease");

        wall_clock.set(Timestamp(1));
        assert!(
            engine
                .confirm_committer_lease_renewal_not_due_for_testing(
                    &tenant_id,
                    Duration::from_secs(1),
                )
                .expect("renewal worker observation should remain available"),
            "the worker must observe the wake and keep the monotonic deadline pending"
        );
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("lease stats should read")
                .committer_lease_renewal_count,
            0,
            "wall-clock movement must not make monotonic renewal due"
        );

        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("renewal worker should wake at the monotonic deadline");
        wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "backward wall-clock step must not delay monotonic renewal",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn lease_renewal_ignores_forward_wall_clock_step() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(200_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = provider_engine_with_lease_clock(
            engine_config,
            wall_clock.clone(),
            lease_clock.clone(),
        )
        .await;
        let tenant_id = TenantId::new("pg-lease-forward-clock").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("acquire"))
            .await
            .expect("first write should acquire the lease");

        wall_clock.set(Timestamp(20_000_000));
        assert!(
            engine
                .confirm_committer_lease_renewal_not_due_for_testing(
                    &tenant_id,
                    Duration::from_secs(1),
                )
                .expect("renewal worker observation should remain available"),
            "the worker must observe the wake and keep the monotonic deadline pending"
        );
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("lease stats should read")
                .committer_lease_renewal_count,
            0,
            "a forward wall-clock step must not trigger an early renewal"
        );

        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("renewal worker should wake at the monotonic deadline");
        wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "forward wall-clock step must not replace the monotonic cadence",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn lease_renewal_shutdown_interrupts_monotonic_wait() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(300_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = provider_engine_with_lease_clock(engine_config, wall_clock, lease_clock).await;
        let tenant_id = TenantId::new("pg-lease-shutdown-wake").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("acquire"))
            .await
            .expect("first write should acquire the lease");
        let runtime = engine
            .registered_runtime_for_testing(&tenant_id)
            .expect("tenant runtime should remain registered");

        let started = std::time::Instant::now();
        runtime.shutdown_committer_lease_renewal();
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "shutdown must wake and join a worker parked before its monotonic deadline; elapsed={elapsed:?}"
        );
        assert!(
            !runtime
                .mutation_journal_stats()
                .committer_lease_renewal_worker_running
        );
        assert_eq!(
            runtime
                .mutation_journal_stats()
                .committer_lease_renewal_count,
            0
        );

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_expiry_remains_authoritative_after_local_clock_divergence() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(400_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = provider_engine_with_lease_clock(
            engine_config,
            wall_clock.clone(),
            lease_clock.clone(),
        )
        .await;
        let tenant_id = TenantId::new("pg-provider-expiry-clock").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("acquire"))
            .await
            .expect("first write should acquire the lease");
        let runtime = engine
            .registered_runtime_for_testing(&tenant_id)
            .expect("tenant runtime should remain registered");

        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("provider lease should expire deterministically");
        wall_clock.set(Timestamp(40_000_000));
        assert!(
            engine
                .confirm_committer_lease_renewal_not_due_for_testing(
                    &tenant_id,
                    Duration::from_secs(1),
                )
                .expect("renewal worker observation should remain available"),
            "provider expiry and wall-clock movement must not bypass the monotonic deadline"
        );
        assert!(
            !runtime.mutation_journal_stats().committer_lease_fenced,
            "local wall-clock movement must not evaluate provider validity"
        );

        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("monotonic deadline should wake provider validation");
        wait_for_value(
            "provider expiry should fence the holder at the monotonic renewal deadline",
            Duration::from_secs(2),
            Duration::ZERO,
            || async { runtime.mutation_journal_stats() },
            |stats| stats.committer_lease_fenced,
        )
        .await;
        assert_eq!(
            runtime
                .mutation_journal_stats()
                .committer_lease_renewal_failure_count,
            1
        );

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_lease_renewal_survives_local_clock_divergence() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(500_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = provider_engine_with_lease_clock(
            engine_config,
            wall_clock.clone(),
            lease_clock.clone(),
        )
        .await;
        let tenant_id = TenantId::new("pg-lease-clock-divergence").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("acquire"))
            .await
            .expect("first write should acquire the lease");

        wall_clock.set(Timestamp(5));
        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("backward-divergent renewal should wake");
        wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "backward-divergent provider renewal should complete",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;

        wall_clock.set(Timestamp(50_000_000));
        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("forward-divergent renewal should wake");
        let renewed = wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "forward-divergent provider renewal should complete",
            |stats| stats.committer_lease_renewal_count == 2,
        )
        .await;
        assert!(renewed.committer_lease_acquired);
        assert!(!renewed.committer_lease_fenced);
        assert_eq!(renewed.committer_lease_epoch, 1);
        assert_eq!(renewed.committer_lease_renewal_failure_count, 0);

        let durable_lease = inspection_store(&provider_config, &tenant_id)
            .await
            .read_committer_lease()
            .expect("durable provider lease should read")
            .expect("durable provider lease should exist");
        assert_eq!(durable_lease.epoch, 1);
        assert_eq!(durable_lease.expires_at, renewed.committer_lease_expires_at);
        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("still-holder"))
            .await
            .expect("renewed holder should remain able to commit");

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn two_postgres_engines_load_without_leases_and_only_one_first_writer_acquires_epoch() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(20_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(20_000)))).await;
        let tenant_id = TenantId::new("pg-two-engine-lease").expect("tenant id should build");
        engine_a
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine_b
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("second engine should load the same tenant");

        let runtime_a = engine_a
            .registered_runtime_for_testing(&tenant_id)
            .expect("first loaded runtime should remain inspectable");
        let runtime_b = engine_b
            .registered_runtime_for_testing(&tenant_id)
            .expect("second loaded runtime should remain inspectable");
        for runtime in [&runtime_a, &runtime_b] {
            let stats = runtime.mutation_journal_stats();
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

        // Inspect the two exact contenders. The losing runtime may already be
        // inside its bounded recovery-eviction window, where a registry lookup
        // correctly reports `Unavailable` instead of lending out that runtime.
        let stats_a = runtime_a.mutation_journal_stats();
        let stats_b = runtime_b.mutation_journal_stats();
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
        let clock_a = Arc::new(ManualWallClock::new(Timestamp(30_000)));
        let clock_b = Arc::new(ManualWallClock::new(Timestamp(30_000)));
        let lease_clock_b = Arc::new(ManualLeaseRenewalClock::new());
        let engine_a = provider_engine(config_a, clock_a).await;
        let engine_b =
            provider_engine_with_lease_clock(config_b, clock_b, lease_clock_b.clone()).await;
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

        lease_clock_b.advance(Duration::from_secs(10));
        let stale_runtime = engine_b
            .registered_runtime_for_testing(&tenant_id)
            .expect("old owner runtime should still be registered before renewal fencing");
        engine_b
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("old owner renewal should wake");
        wait_for_value(
            "old owner renewal should record fencing",
            Duration::from_secs(1),
            Duration::ZERO,
            || async { stale_runtime.mutation_journal_stats() },
            |stats| stats.committer_lease_fenced,
        )
        .await;
        tokio::time::timeout(
            Duration::from_secs(5),
            stale_runtime.wait_for_eviction_complete(),
        )
        .await
        .expect("renewal-fenced runtime eviction should complete");
        let replacement_identity = engine_b
            .get_existing_tenant_async_for_testing(&tenant_id)
            .await
            .expect("renewal fencing should evict and reload the provider runtime");
        assert_ne!(replacement_identity, Arc::as_ptr(&stale_runtime) as usize);
        assert!(
            stale_runtime
                .mutation_journal_stats()
                .committer_lease_fenced
        );
        assert_eq!(
            stale_runtime
                .mutation_journal_stats()
                .committer_lease_renewal_failure_count,
            1
        );
        assert!(
            !engine_b.runtime_is_registered_for_testing(&tenant_id, &stale_runtime),
            "a renewal CAS fence must surrender the stale runtime's sequence authority"
        );

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_fences_every_provider_record_writer_without_partial_persistence() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(40_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(40_000)))).await;

        // Queued async batch path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-queued").await;
        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("old-holder"))
            .await
            .expect("old holder queued write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("queued lease should expire");
        engine_b
            .insert_document_async(tenant_id.clone(), tasks_table(), title("healthy-holder"))
            .await
            .expect("healthy queued holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("queued head should read");
        let fenced_id = DocumentId::new();
        assert_terminal_fenced(
            engine_a
                .insert_document_async_with_id(
                    tenant_id.clone(),
                    tasks_table(),
                    fenced_id.clone(),
                    title("must-not-persist"),
                )
                .await
                .expect_err("stale queued holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("queued head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("queued document lookup should succeed")
                .is_none()
        );

        // Direct synchronous prepared-commit path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-direct").await;
        engine_a
            .insert_document(&tenant_id, tasks_table(), title("old-holder"))
            .expect("old holder direct write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("direct lease should expire");
        engine_b
            .insert_document(&tenant_id, tasks_table(), title("healthy-holder"))
            .expect("healthy direct holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("direct head should read");
        let fenced_id = DocumentId::new();
        assert_terminal_fenced(
            engine_a
                .insert_document_with_id(
                    &tenant_id,
                    tasks_table(),
                    fenced_id.clone(),
                    title("must-not-persist"),
                )
                .expect_err("stale direct holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("direct head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("direct document lookup should succeed")
                .is_none()
        );
        // A lease CAS rejection is settled inside the write transaction, so it
        // proves rollback on its own. Probing durable progress could only turn
        // a certain outcome into an uncertain one, so the classifier must not
        // probe at all on this path.
        assert_eq!(
            engine_a.durable_outcome_probe_count_for_testing(&tenant_id, DurableWriteRoute::Direct),
            0,
            "a committer fence must be classified definitive without probing durable progress"
        );

        // Mutation execution-unit path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-execution-unit").await;
        let old_unit = engine_a
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("old holder execution unit should begin");
        old_unit
            .insert_document(tasks_table(), title("old-holder"))
            .expect("old holder write should stage");
        old_unit
            .commit()
            .expect("old holder execution unit should acquire and commit");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("execution-unit lease should expire");
        let healthy_unit = engine_b
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("healthy execution unit should begin");
        healthy_unit
            .insert_document(tasks_table(), title("healthy-holder"))
            .expect("healthy write should stage");
        healthy_unit
            .commit()
            .expect("healthy execution holder should take over and commit");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("execution head should read");
        let fenced_id = DocumentId::new();
        let fenced_unit = engine_a
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("stale execution unit should begin");
        fenced_unit
            .insert_document_with_id(
                tasks_table(),
                Some(fenced_id.clone()),
                title("must-not-persist"),
            )
            .expect("stale write should stage before fencing");
        assert_terminal_fenced(
            fenced_unit
                .commit()
                .expect_err("stale execution-unit holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("execution head should reread"),
            before
        );
        assert!(
            store
                .get(&tasks_table(), &fenced_id)
                .expect("execution document lookup should succeed")
                .is_none()
        );
        assert_eq!(
            engine_a.durable_outcome_probe_count_for_testing(
                &tenant_id,
                DurableWriteRoute::ExecutionUnit,
            ),
            0,
            "a fenced execution unit must be definitive without a durable-progress probe"
        );

        // Schema set path (delete shares the same fenced schema transaction seam).
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-schema").await;
        engine_a
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("old holder schema write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("schema lease should expire");
        let mut healthy_schema = tasks_schema();
        healthy_schema.fields.push(FieldSchema {
            name: "healthy".to_string(),
            field_type: FieldType::String,
            required: false,
        });
        engine_b
            .set_table_schema_async(tenant_id.clone(), healthy_schema.clone())
            .await
            .expect("healthy schema holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("schema head should read");
        let mut fenced_schema = tasks_schema();
        fenced_schema.fields.push(FieldSchema {
            name: "fenced".to_string(),
            field_type: FieldType::String,
            required: false,
        });
        assert_terminal_fenced(
            engine_a
                .set_table_schema_async(tenant_id.clone(), fenced_schema)
                .await
                .expect_err("stale schema holder must be fenced"),
        );
        assert_eq!(
            store.journal_progress().expect("schema head should reread"),
            before
        );
        assert_eq!(
            store
                .load_schema()
                .expect("persisted schema should read")
                .get_table(&tasks_table()),
            Some(&healthy_schema)
        );
        assert_eq!(
            engine_a
                .durable_outcome_probe_count_for_testing(&tenant_id, DurableWriteRoute::SchemaSet,),
            0,
            "a fenced schema set must be definitive without a durable-progress probe"
        );

        // Schema delete uses its own provider transaction entry point.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-schema-delete").await;
        engine_a
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("old holder should seed schema before delete");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("schema-delete lease should expire");
        engine_b
            .delete_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("healthy schema-delete holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("schema-delete head should read");
        assert_terminal_fenced(
            engine_a
                .delete_table_schema_async(tenant_id.clone(), tasks_table())
                .await
                .expect_err("stale schema-delete holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("schema-delete head should reread"),
            before
        );
        assert!(
            store
                .load_schema()
                .expect("schema after delete should read")
                .get_table(&tasks_table())
                .is_none()
        );
        assert_eq!(
            engine_a.durable_outcome_probe_count_for_testing(
                &tenant_id,
                DurableWriteRoute::SchemaDelete,
            ),
            0,
            "a fenced schema delete must be definitive without a durable-progress probe"
        );

        // Internal trigger-materialization/cursor path.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-internal").await;
        engine_a
            .materialize_trigger_cursor_for_testing(
                &tenant_id,
                TriggerDeliveryCursor::new(SequenceNumber(1)),
            )
            .expect("old holder internal cursor write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("internal lease should expire");
        engine_b
            .materialize_trigger_cursor_for_testing(
                &tenant_id,
                TriggerDeliveryCursor::new(SequenceNumber(2)),
            )
            .expect("healthy internal holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store.journal_progress().expect("internal head should read");
        assert_terminal_fenced(
            engine_a
                .materialize_trigger_cursor_for_testing(
                    &tenant_id,
                    TriggerDeliveryCursor::new(SequenceNumber(3)),
                )
                .expect_err("stale internal holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("internal head should reread"),
            before
        );
        assert_eq!(
            store
                .trigger_delivery_cursor()
                .expect("trigger cursor should read"),
            TriggerDeliveryCursor::new(SequenceNumber(2))
        );

        // Ordered publisher persistence seam, exercised directly because provider
        // publisher authority is enabled by the later slice-C topology cleanup.
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-publisher").await;
        engine_a
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "old-holder")
            .expect("old holder publisher write should acquire");
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("publisher lease should expire");
        engine_b
            .persist_provider_publisher_barrier_for_testing(&tenant_id, "healthy-holder")
            .expect("healthy publisher holder should take over and write");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before = store
            .journal_progress()
            .expect("publisher head should read");
        assert_terminal_fenced(
            engine_a
                .persist_provider_publisher_barrier_for_testing(&tenant_id, "must-not-persist")
                .expect_err("stale publisher holder must be fenced"),
        );
        assert_eq!(
            store
                .journal_progress()
                .expect("publisher head should reread"),
            before
        );
        assert!(
            store
                .read_durable_journal_from(SequenceNumber(before.durable_head.0.saturating_add(1)))
                .expect("publisher suffix should read")
                .is_empty()
        );
        assert_eq!(
            engine_a
                .durable_outcome_probe_count_for_testing(&tenant_id, DurableWriteRoute::Publisher,),
            0,
            "a first-attempt publisher fence proves rollback without a progress probe"
        );

        // Point-in-time restore journal import, which sits outside the mutation API.
        let source_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-source").await;
        engine_a
            .set_table_schema_async(source_id.clone(), tasks_schema())
            .await
            .expect("restore source schema should persist");
        let restored_document_id = DocumentId::new();
        engine_a
            .insert_document_async_with_id(
                source_id.clone(),
                tasks_table(),
                restored_document_id.clone(),
                title("restored"),
            )
            .await
            .expect("restore source document should persist");
        let archive = engine_a
            .export_latest_point_in_time_restore_archive(&source_id)
            .expect("restore archive should export");

        let healthy_id =
            create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-healthy").await;
        engine_a
            .import_point_in_time_restore_archive(&healthy_id, &archive)
            .expect("healthy restore holder should import through the fence");
        let healthy_store = inspection_store(&provider_config, &healthy_id).await;
        assert_eq!(
            healthy_store
                .journal_progress()
                .expect("healthy restore progress should read")
                .applied_head,
            archive.target_sequence
        );
        assert!(
            healthy_store
                .get(&tasks_table(), &restored_document_id)
                .expect("healthy restored document should read")
                .is_some()
        );

        let fenced_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-restore-stale").await;
        engine_a
            .acquire_committer_lease_for_testing(&fenced_id)
            .expect("old restore holder should acquire without populating the tenant");
        expire_postgres_committer_lease(&provider_config, &fenced_id)
            .await
            .expect("restore lease should expire");
        engine_b
            .acquire_committer_lease_for_testing(&fenced_id)
            .expect("healthy restore holder should take over the empty tenant");
        let fenced_store = inspection_store(&provider_config, &fenced_id).await;
        let lease_before = fenced_store
            .read_committer_lease()
            .expect("restore lease should read");
        assert_terminal_fenced(
            engine_a
                .import_point_in_time_restore_archive(&fenced_id, &archive)
                .expect_err("stale restore holder must be fenced"),
        );
        let fenced_progress = fenced_store
            .journal_progress()
            .expect("fenced restore progress should read");
        assert_eq!(fenced_progress.durable_head, SequenceNumber(0));
        assert_eq!(fenced_progress.applied_head, SequenceNumber(0));
        assert!(
            fenced_store
                .load_schema()
                .expect("fenced restore schema should read")
                .tables
                .is_empty()
        );
        assert!(
            fenced_store
                .get(&tasks_table(), &restored_document_id)
                .expect("fenced restored document lookup should succeed")
                .is_none()
        );
        assert_eq!(
            fenced_store
                .read_committer_lease()
                .expect("restore lease should reread"),
            lease_before,
            "a rejected restore must not alter the healthy holder's lease"
        );

        for tenant_id in [
            "pg-fence-queued",
            "pg-fence-direct",
            "pg-fence-execution-unit",
            "pg-fence-schema",
            "pg-fence-schema-delete",
            "pg-fence-internal",
            "pg-fence-publisher",
            "pg-fence-restore-stale",
        ] {
            let tenant_id = TenantId::new(tenant_id).expect("tenant id should rebuild");
            engine_a
                .get_existing_tenant_async_for_testing(&tenant_id)
                .await
                .expect("fenced provider runtime should evict and reload");
            assert_eq!(
                engine_a
                    .publisher_failure_diagnostics_for_testing(&tenant_id)
                    .expect("fence eviction should preserve runtime diagnostics")
                    .2,
                0,
                "definitive fencing must not increment ambiguous crash-replay diagnostics"
            );
            let stats = engine_a
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("replacement runtime stats should read");
            assert!(!stats.committer_lease_acquired);
            assert!(!stats.committer_lease_fenced);
        }

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_fence_eviction_reloads_without_unfenced_fallback_or_ping_pong() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(50_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(50_000)))).await;
        let tenant_id = create_shared_tenant(&engine_a, &engine_b, "pg-fence-evict").await;

        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("old-holder"))
            .await
            .expect("old holder should acquire epoch one");
        let stale_runtime = engine_a
            .registered_runtime_for_testing(&tenant_id)
            .expect("old holder runtime should be registered");
        let stale_identity = Arc::as_ptr(&stale_runtime) as usize;

        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("old holder lease should expire deterministically");
        let healthy_id = DocumentId::new();
        engine_b
            .insert_document_async_with_id(
                tenant_id.clone(),
                tasks_table(),
                healthy_id.clone(),
                title("healthy-holder"),
            )
            .await
            .expect("healthy holder should acquire epoch two");

        let rejected_id = DocumentId::new();
        assert_terminal_fenced(
            engine_a
                .insert_document_async_with_id(
                    tenant_id.clone(),
                    tasks_table(),
                    rejected_id.clone(),
                    title("must-not-persist"),
                )
                .await
                .expect_err("stale holder must lose its definitive CAS"),
        );

        tokio::time::timeout(
            Duration::from_secs(5),
            stale_runtime.wait_for_eviction_complete(),
        )
        .await
        .expect("fenced runtime eviction should complete before replacement inspection");
        let replacement_identity = engine_a
            .get_existing_tenant_async_for_testing(&tenant_id)
            .await
            .expect("fenced runtime should be replaced");
        assert_ne!(replacement_identity, stale_identity);
        assert!(!engine_a.runtime_is_registered_for_testing(&tenant_id, &stale_runtime));
        assert_eq!(
            engine_a
                .publisher_failure_diagnostics_for_testing(&tenant_id)
                .expect("fence eviction diagnostics should persist")
                .2,
            0,
            "the definitive CAS rollback is never an ambiguous outcome"
        );

        let store = inspection_store(&provider_config, &tenant_id).await;
        assert!(
            store
                .get(&tasks_table(), &healthy_id)
                .expect("healthy record should read")
                .is_some()
        );
        assert!(
            store
                .get(&tasks_table(), &rejected_id)
                .expect("rejected record lookup should succeed")
                .is_none(),
            "the fenced writer's rolled-back record must never appear"
        );

        // The replacement is lazy and unacquired. Repeated attempts while B
        // legitimately holds epoch two fail at acquisition and keep the same
        // runtime; they neither write unfenced nor evict the rightful holder.
        for attempt in 0..3 {
            let contender_id = DocumentId::new();
            let error = engine_a
                .insert_document_async_with_id(
                    tenant_id.clone(),
                    tasks_table(),
                    contender_id.clone(),
                    title("still-not-holder"),
                )
                .await
                .expect_err("an unexpired healthy lease must defeat every contender attempt");
            assert!(
                !matches!(error, nimbus_core::Error::CommitterFenced { .. }),
                "a freshly reloaded contender has no stale token to fence"
            );
            assert!(
                store
                    .get(&tasks_table(), &contender_id)
                    .expect("contender record lookup should succeed")
                    .is_none()
            );
            assert_eq!(
                engine_a
                    .get_existing_tenant_async_for_testing(&tenant_id)
                    .await
                    .expect("contender runtime should remain serviceable"),
                replacement_identity,
                "held-lease contention attempt {attempt} must not ping-pong runtimes"
            );
        }

        engine_b
            .insert_document_async(tenant_id.clone(), tasks_table(), title("still-healthy"))
            .await
            .expect("rightful epoch-two holder should remain serviceable");

        // A handoff occurs only after explicit expiry. Then A legitimately
        // acquires epoch three; no automatic eviction loop can manufacture it.
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("healthy holder lease should expire for an intentional handoff");
        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("rightful-again"))
            .await
            .expect("replacement runtime should lazily acquire epoch three");
        assert_eq!(
            engine_a
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("replacement lease stats should read")
                .committer_lease_epoch,
            3
        );

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}
