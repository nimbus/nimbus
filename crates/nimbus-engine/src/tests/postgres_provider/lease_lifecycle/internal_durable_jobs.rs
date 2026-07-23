use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_schedule_only_execution_unit_rejects_stale_provider_holder() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(41_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(41_000)))).await;
        exercise_provider_schedule_only_execution_unit_fence_contract(
            engine_a,
            engine_b,
            TenantId::new("pg-fence-schedule-only-unit").expect("tenant id should build"),
            Arc::new(PostgresLeaseTimeControl::new(provider_config)),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_schedule_only_execution_unit_reconciles_acknowledgement_loss() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let faults = Arc::new(ArmedProviderCommitAcknowledgementLoss::default());
        let engine = provider_engine_with_faults(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(43_000))),
            faults.clone(),
        )
        .await;
        let tenant_id = TenantId::new("pg-schedule-only-ack-loss").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let unit = engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("schedule-only execution unit should begin");
        let job_id = unit
            .schedule_mutation_at(
                Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: title("landed-without-ack"),
                },
                100_000,
            )
            .expect("schedule-only operation should stage");
        faults.arm();
        assert_eq!(
            unit.commit()
                .expect("exact scheduler state should reconcile the lost acknowledgement"),
            None,
            "schedule-only execution units never allocate journal records"
        );
        assert!(
            faults.fired.load(std::sync::atomic::Ordering::Acquire),
            "the post-visibility acknowledgement-loss fault must fire exactly once"
        );

        let store = inspection_store(&provider_config, &tenant_id).await;
        assert_eq!(
            store
                .journal_progress()
                .expect("provider progress should remain readable"),
            nimbus_storage::JournalProgress {
                durable_head: SequenceNumber(0),
                applied_head: SequenceNumber(0),
            },
            "schedule-only acknowledgement reconciliation must not advance mutation heads"
        );
        let jobs = store
            .list_scheduled_jobs()
            .expect("scheduled jobs should remain readable");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, job_id);

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_stale_trigger_outcome_cannot_overwrite_successor_state() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(45_000)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(45_000)))).await;
        exercise_provider_trigger_invocation_fence_contract(
            engine_a,
            engine_b,
            TenantId::new("pg-fence-trigger-lifecycle").expect("tenant id should build"),
            Arc::new(PostgresLeaseTimeControl::new(provider_config)),
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_trigger_transition_serializes_with_same_owner_journal_commit() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let faults = BlockingFaultInjector::new(FaultPoint::TriggerTransitionAfterHeadObservation);
        let engine = provider_engine_with_faults(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(46_000))),
            faults.clone(),
        )
        .await;
        exercise_provider_trigger_transition_serialization_contract(
            engine,
            TenantId::new("pg-trigger-journal-serialization").expect("tenant id should build"),
            faults,
        )
        .await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_trigger_outcome_reconciles_acknowledgement_loss_without_reexecution() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let faults = Arc::new(ArmedProviderCommitAcknowledgementLoss::default());
        let engine = provider_engine_with_faults(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(47_000))),
            faults.clone(),
        )
        .await;
        let faults_for_arm = faults.clone();
        let faults_for_assertion = faults.clone();
        exercise_provider_trigger_outcome_acknowledgement_loss_contract(
            engine,
            TenantId::new("pg-trigger-outcome-ack-loss").expect("tenant id should build"),
            move || faults_for_arm.arm(),
            move || {
                faults_for_assertion
                    .fired
                    .load(std::sync::atomic::Ordering::Acquire)
            },
        )
        .await;
    })
    .await;
}
