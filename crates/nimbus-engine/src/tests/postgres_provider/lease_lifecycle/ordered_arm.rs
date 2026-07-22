use super::*;

async fn create_shared_ordered_tenant(
    engine_a: &Arc<Engine>,
    engine_b: &Arc<Engine>,
    name: &str,
) -> TenantId {
    let tenant_id = TenantId::new(name).expect("tenant id should build");
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("first ordered engine should create tenant");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("second ordered engine should load tenant without acquiring");
    tenant_id
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn process_local_sequence_authority_does_not_select_committer_arm() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let embedded_dir = tempdir().expect("embedded arm tempdir should create");
        let embedded = Engine::new(embedded_dir.path()).expect("embedded engine should create");
        let embedded_tenant =
            TenantId::new("embedded-static-arm").expect("embedded tenant id should build");
        embedded
            .create_tenant(embedded_tenant.clone())
            .expect("embedded synchronous lifecycle should create tenant");
        assert_eq!(
            embedded
                .mutation_journal_stats_for_testing(&embedded_tenant)
                .expect("embedded arm diagnostics should load")
                .committer_arm,
            crate::tenant::CommitterArm::OrderedPublisher
        );

        let provider = provider_engine(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(9_000))),
        )
        .await;
        let provider_tenant =
            TenantId::new("provider-static-arm").expect("provider tenant id should build");
        provider
            .create_tenant_async(provider_tenant.clone())
            .await
            .expect("provider async lifecycle should create tenant");
        assert_eq!(
            provider
                .mutation_journal_stats_for_testing(&provider_tenant)
                .expect("provider arm diagnostics should load")
                .committer_arm,
            crate::tenant::CommitterArm::OrderedPublisher,
            "provider topology must install the production publisher"
        );
        assert!(
            !provider
                .registered_runtime_for_testing(&provider_tenant)
                .expect("provider runtime should remain registered")
                .store
                .has_process_local_sequence_authority(),
            "provider window trust must remain storage-backed after the arm flip"
        );

        embedded.quiesce().await;
        provider.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_pipeline_acquires_lease_before_assignment() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let engine = provider_engine(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(9_100))),
        )
        .await;
        let tenant_id =
            TenantId::new("pg-ordered-first-assignment").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("ordered provider tenant should create");
        let loaded = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("loaded diagnostics should read");
        assert_eq!(
            loaded.committer_arm,
            crate::tenant::CommitterArm::OrderedPublisher
        );
        assert!(!loaded.committer_lease_acquired);
        assert_eq!(loaded.committer_lease_acquire_count, 0);

        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("first"))
            .await
            .expect("first ordered provider write should acquire and commit");
        let acquired = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("acquired diagnostics should read");
        assert!(acquired.committer_lease_acquired);
        assert_eq!(acquired.committer_lease_epoch, 1);
        assert_eq!(acquired.committer_lease_acquire_count, 1);
        assert!(acquired.durable_head >= SequenceNumber(1));

        engine
            .insert_document_async(tenant_id.clone(), tasks_table(), title("already-held"))
            .await
            .expect("later ordered provider write should reuse the lease");
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("reused diagnostics should read")
                .committer_lease_acquire_count,
            1,
            "an already-held provider lease must not perform acquisition I/O"
        );
        assert_eq!(
            inspection_store(&provider_config, &tenant_id)
                .await
                .read_committer_lease()
                .expect("provider lease should read")
                .expect("provider lease should exist")
                .epoch,
            1
        );

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_pipeline_acquire_failure_stages_no_suffix() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(9_200)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(9_200)))).await;
        let tenant_id =
            create_shared_ordered_tenant(&engine_a, &engine_b, "pg-ordered-acquire-failure").await;
        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("provider hints should stop before the ownership test");
        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("holder"))
            .await
            .expect("first engine should acquire the provider lease");
        engine_a
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("holder trigger work should settle before inspection");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let before_progress = store
            .journal_progress()
            .expect("provider progress should read before contention");
        let before_assignment = engine_b
            .write_log_assignment_for_testing(&tenant_id)
            .expect("contender assignment should read");

        let error = engine_b
            .insert_document_async(tenant_id.clone(), tasks_table(), title("must-not-stage"))
            .await
            .expect_err("an unexpired holder must reject the ordered contender");
        assert_eq!(
            error.storage_kind(),
            Some(nimbus_core::StorageErrorKind::Busy),
            "lease contention must preserve its typed provider error"
        );

        assert_eq!(
            engine_b
                .write_log_assignment_for_testing(&tenant_id)
                .expect("contender assignment should reread"),
            before_assignment,
            "lease admission failure must not stage or assign a suffix"
        );
        assert!(before_assignment.1.is_empty());
        assert_eq!(
            store
                .journal_progress()
                .expect("provider progress should reread"),
            before_progress,
            "lease admission failure must leave durable provider progress unchanged"
        );
        let failed = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("contender diagnostics should read");
        assert!(!failed.committer_lease_acquired);
        assert_eq!(failed.committer_lease_acquire_count, 0);
        assert_eq!(failed.worker_failure_count, 1);

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_pipeline_head_reconciles_before_baseline_capture() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(
                config_a,
                Arc::new(ManualWallClock::new(Timestamp(9_300))),
            )
            .await;
        let engine_b =
            provider_engine(
                config_b,
                Arc::new(ManualWallClock::new(Timestamp(9_300))),
            )
            .await;
        let tenant_id =
            create_shared_ordered_tenant(&engine_a, &engine_b, "pg-ordered-reconcile").await;
        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("provider hints should stop before predecessor persistence");
        engine_a
            .insert_document_async(tenant_id.clone(), tasks_table(), title("predecessor"))
            .await
            .expect("predecessor should acquire and commit");
        engine_a
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("predecessor trigger work should settle");
        let store = inspection_store(&provider_config, &tenant_id).await;
        let predecessor_head = store
            .journal_progress()
            .expect("predecessor progress should read")
            .durable_head;
        assert!(predecessor_head >= SequenceNumber(1));
        assert_eq!(
            engine_b
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("stale contender diagnostics should read")
                .durable_head,
            SequenceNumber(0),
            "the test requires a runtime loaded before predecessor progress"
        );

        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("predecessor lease should expire");
        let successor_id = engine_b
            .insert_document_async(tenant_id.clone(), tasks_table(), title("successor"))
            .await
            .expect("successor should reconcile, assign, and commit");
        engine_b
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("successor trigger work should settle");
        let successor_sequence = store
            .read_durable_journal_from(SequenceNumber(0))
            .expect("provider journal should read")
            .into_iter()
            .find(|record| {
                record
                    .writes
                    .iter()
                    .any(|write| write.doc_id == successor_id)
            })
            .expect("successor record should be present")
            .sequence;
        assert_eq!(
            successor_sequence,
            SequenceNumber(predecessor_head.0 + 1),
            "assignment baseline must be captured after acquisition republishes predecessor progress"
        );
        let reconciled = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("reconciled diagnostics should read");
        assert_eq!(reconciled.committer_lease_epoch, 2);
        assert!(reconciled.durable_head >= successor_sequence);

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn production_provider_queued_mutation_reaches_ordered_publisher() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let engine = provider_engine(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(9_350))),
        )
        .await;
        let tenant_id = TenantId::new("pg-ordered-progress-order").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("ordered provider tenant should create");
        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("trigger cursor should not add unrelated records");

        let faults = engine.commit_fault_handle_for_testing();
        let pause = labels::POST_PUBLISH_PRE_FANOUT;
        faults.arm(pause);
        let write = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        title("publisher-before-progress"),
                    )
                    .await
            }
        });
        let entered = tokio::task::spawn_blocking({
            let faults = faults.clone();
            move || faults.wait_until_entered(pause, Duration::from_secs(5))
        })
        .await
        .expect("publisher pause wait should join");
        assert!(entered, "ordered provider publisher should reach the pause");

        let mut progress_sync = tokio::task::spawn_blocking({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            move || {
                engine.sync_mutation_journal_progress_for_testing(
                    &tenant_id,
                    nimbus_storage::JournalProgress {
                        durable_head: SequenceNumber(2),
                        applied_head: SequenceNumber(2),
                    },
                )
            }
        });
        assert!(
            timeout(Duration::from_millis(100), &mut progress_sync)
                .await
                .is_err(),
            "provider progress sync must queue behind the construction-selected publisher"
        );

        faults.release(pause);
        timeout(Duration::from_secs(5), write)
            .await
            .expect("ordered provider write should finish after release")
            .expect("ordered provider write task should join")
            .expect("ordered provider write should succeed");
        timeout(Duration::from_secs(5), progress_sync)
            .await
            .expect("provider progress sync should drain after publisher")
            .expect("provider progress sync task should join")
            .expect("provider progress sync should succeed");

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn two_provider_engines_never_assign_under_one_tenant_lease() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(9_400)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(9_400)))).await;
        let tenant_id =
            create_shared_ordered_tenant(&engine_a, &engine_b, "pg-ordered-one-owner").await;
        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("provider hints should stop before concurrent assignment");

        let write_a =
            engine_a.insert_document_async(tenant_id.clone(), tasks_table(), title("contender-a"));
        let write_b =
            engine_b.insert_document_async(tenant_id.clone(), tasks_table(), title("contender-b"));
        let (result_a, result_b) = tokio::join!(write_a, write_b);
        assert_eq!(
            usize::from(result_a.is_ok()) + usize::from(result_b.is_ok()),
            1,
            "exactly one provider runtime may acquire and assign"
        );
        let losing_engine = if result_a.is_err() {
            &engine_a
        } else {
            &engine_b
        };
        let (_, losing_pending) = losing_engine
            .write_log_assignment_for_testing(&tenant_id)
            .expect("losing assignment state should read");
        assert!(
            losing_pending.is_empty(),
            "the runtime that lost lease admission must own no pending suffix"
        );
        let lease = inspection_store(&provider_config, &tenant_id)
            .await
            .read_committer_lease()
            .expect("provider lease should read")
            .expect("one provider lease should exist");
        assert_eq!(lease.epoch, 1);

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_pipeline_cancellation_before_lease_admission_stages_no_suffix() {
    with_shared_postgres_engine_configs(|config_a, config_b, provider_config| async move {
        let engine_a =
            provider_engine(config_a, Arc::new(ManualWallClock::new(Timestamp(9_500)))).await;
        let engine_b =
            provider_engine(config_b, Arc::new(ManualWallClock::new(Timestamp(9_500)))).await;
        let tenant_id =
            create_shared_ordered_tenant(&engine_a, &engine_b, "pg-ordered-cancel").await;
        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("provider hints should stop before cancellation");
        let pause = engine_b
            .mutation_journal_pause_handle_for_testing(&tenant_id)
            .expect("ordered contender pause should load");
        pause.arm();
        let cancel = Arc::new(Notify::new());
        let cancel_for_wait = cancel.clone();
        let write = tokio::spawn({
            let engine_b = engine_b.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine_b
                    .insert_document_async_with(
                        tenant_id,
                        tasks_table(),
                        None,
                        title("cancelled-before-lease"),
                        crate::AsyncMutationContext::anonymous(
                            async move { cancel_for_wait.notified().await },
                            || Ok(()),
                        ),
                    )
                    .await
            }
        });
        let entered = tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(5))
        })
        .await
        .expect("pause wait should join");
        assert!(entered, "ordered batch should pause before lease admission");
        cancel.notify_one();
        timeout(
            Duration::from_secs(5),
            engine_b.wait_for_queued_mutation_cancellation_observed_for_testing(&tenant_id),
        )
        .await
        .expect("queued cancellation should be observed before lease admission resumes")
        .expect("queued cancellation observation should succeed");
        pause.release();
        let error = timeout(Duration::from_secs(5), write)
            .await
            .expect("cancelled provider write should resolve")
            .expect("cancelled provider task should join")
            .expect_err("cancelled provider write must not succeed");
        assert!(matches!(error, nimbus_core::Error::Cancelled));
        let stats = engine_b
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("cancelled diagnostics should read");
        assert_eq!(stats.committer_lease_acquire_count, 0);
        assert!(
            engine_b
                .write_log_assignment_for_testing(&tenant_id)
                .expect("cancelled assignment should read")
                .1
                .is_empty()
        );
        assert_eq!(
            inspection_store(&provider_config, &tenant_id)
                .await
                .journal_progress()
                .expect("cancelled provider progress should read")
                .durable_head,
            SequenceNumber(0)
        );

        engine_a.quiesce().await;
        engine_b.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn provider_pipeline_shutdown_before_lease_admission_stages_no_suffix() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let engine = provider_engine(
            engine_config,
            Arc::new(ManualWallClock::new(Timestamp(9_600))),
        )
        .await;
        let tenant_id = TenantId::new("pg-ordered-shutdown").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("ordered provider tenant should create");
        let pause = engine
            .mutation_journal_pause_handle_for_testing(&tenant_id)
            .expect("ordered shutdown pause should load");
        pause.arm();
        let write = tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(tenant_id, tasks_table(), title("must-not-start"))
                    .await
            }
        });
        let entered = tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(5))
        })
        .await
        .expect("pause wait should join");
        assert!(
            entered,
            "ordered batch should pause before shutdown admission"
        );
        let runtime = engine
            .registered_runtime_for_testing(&tenant_id)
            .expect("ordered runtime should remain registered");
        runtime.shutdown_committer();
        assert!(runtime.committer_shutdown_token().is_cancelled());
        pause.release();
        let error = timeout(Duration::from_secs(5), write)
            .await
            .expect("shutdown provider write should resolve")
            .expect("shutdown provider task should join")
            .expect_err("shutdown provider write must not succeed");
        assert!(matches!(error, nimbus_core::Error::Cancelled));
        assert_eq!(
            runtime
                .mutation_journal_stats()
                .committer_lease_acquire_count,
            0
        );
        assert!(
            engine
                .write_log_assignment_for_testing(&tenant_id)
                .expect("shutdown assignment should read")
                .1
                .is_empty()
        );
        assert_eq!(
            inspection_store(&provider_config, &tenant_id)
                .await
                .journal_progress()
                .expect("shutdown provider progress should read")
                .durable_head,
            SequenceNumber(0)
        );

        engine.quiesce().await;
    })
    .await;
}
