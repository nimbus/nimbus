use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-mutations").expect("tenant id should build");
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("postgres-backed engine should create"),
        );

        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");

        let inserted_id = engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("First"))]),
            )
            .await
            .expect("insert should succeed");
        engine
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                inserted_id,
                serde_json::Map::from_iter([("title".to_string(), json!("Renamed"))]),
            )
            .await
            .expect("update should succeed");

        let scheduled_job_id = engine
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 5_000,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
                        id: None,
                        fields: serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("Scheduled"),
                        )]),
                    },
                },
            )
            .await
            .expect("scheduled mutation should persist");
        assert_eq!(
            engine
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        let claimed = engine
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        engine
            .record_scheduled_job_result_async(
                tenant_id.clone(),
                nimbus_core::ScheduledJobResult {
                    id: scheduled_job_id.clone(),
                    run_at: claimed[0].run_at,
                    finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                    mutation: claimed[0].mutation.clone(),
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .await
            .expect("scheduled result should persist");
        engine
            .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id.clone())
            .await
            .expect("scheduled completion should persist");
        assert_eq!(
            engine
                .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id.clone())
                .await
                .expect("scheduled result should load")
                .outcome,
            ScheduledJobOutcome::Completed
        );

        let documents = engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("query should succeed");
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0]
                .fields
                .get("title")
                .and_then(|value| value.as_str()),
            Some("Renamed")
        );

        let bootstrap = engine
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        let latest_sequence = engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("latest sequence should load");
        assert_eq!(bootstrap.bootstrap_cut, latest_sequence);
        assert_eq!(bootstrap.resume_after, latest_sequence);

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_notifications_refresh_loaded_runtime_schema_and_journal_state() {
    with_shared_postgres_engine_configs(
        |engine_config_a, engine_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-journal").expect("tenant id should build");
            let engine_a = Arc::new(
                Engine::new_with_persistence_config(engine_config_a)
                    .await
                    .expect("first postgres-backed engine should create"),
            );
            let engine_b = Arc::new(
                Engine::new_with_persistence_config(engine_config_b)
                    .await
                    .expect("second postgres-backed engine should create"),
            );

            engine_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            engine_b
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("second engine should load tenant");
            assert_eq!(
                engine_b
                    .get_schema_async(tenant_id.clone())
                    .await
                    .expect("empty schema should load"),
                Schema::default()
            );

            engine_a
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema write should succeed");
            engine_a
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("External"))]),
                )
                .await
                .expect("insert should succeed");

            wait_for_value(
                "postgres notification should refresh loaded schema",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let engine = engine_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        engine
                            .get_schema_async(tenant_id)
                            .await
                            .expect("schema should load")
                    }
                },
                |schema| schema.get_table(&tasks_table()).is_some(),
            )
            .await;
            wait_for_mutation_journal_stats(
                &engine_b,
                &tenant_id,
                "postgres notification should catch up journal heads",
                |stats| stats.durable_head.0 >= 2 && stats.applied_head.0 >= 2,
            )
            .await;

            let documents = engine_b
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("caught-up query should succeed");
            assert_eq!(documents.len(), 1);
            assert_eq!(
                documents[0]
                    .fields
                    .get("title")
                    .and_then(|value| value.as_str()),
                Some("External")
            );

            tokio::time::timeout(Duration::from_secs(2), engine_a.quiesce())
                .await
                .expect("first engine should quiesce after reconnect test");
            tokio::time::timeout(Duration::from_secs(2), engine_b.quiesce())
                .await
                .expect("second engine should quiesce after reconnect test");
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_listener_reconnect_recovers_missed_schema_and_journal_hints() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-notify-reconnect").expect("tenant id should build");
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("postgres-backed engine should create"),
        );

        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("engine should load tenant");
        assert_eq!(
            engine
                .get_schema_async(tenant_id.clone())
                .await
                .expect("empty schema should load"),
            Schema::default()
        );
        let original_listener_pids = list_postgres_hint_listener_pids(&provider_config)
            .await
            .expect("listener pid list should load");
        assert!(
            !original_listener_pids.is_empty(),
            "expected at least one hint listener backend before reconnect drill"
        );
        let original_listener_pids = original_listener_pids.into_iter().collect::<BTreeSet<_>>();

        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("listener termination should succeed");

        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("external provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        opened
            .store
            .replace_table_schema(&tasks_schema())
            .expect("external schema write should succeed");
        opened
            .store
            .insert(&Document {
                table: tasks_table(),
                id: DocumentId::new(),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("Recovered"))]),
                creation_time: Timestamp(100),
                update_time: Timestamp(100),
                typed_fields: Default::default(),
            })
            .expect("external document write should succeed");

        wait_for_value(
            "postgres reconnect should restore a new hint listener backend",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                let original_listener_pids = original_listener_pids.clone();
                async move {
                    let current = list_postgres_hint_listener_pids(&provider_config)
                        .await
                        .expect("listener pid list should load");
                    current
                        .into_iter()
                        .any(|pid| !original_listener_pids.contains(&pid))
                }
            },
            |restored| *restored,
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed schema changes",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    engine
                        .get_schema_async(tenant_id)
                        .await
                        .expect("schema should load")
                }
            },
            |schema| schema.get_table(&tasks_table()).is_some(),
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed journal commits",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    engine
                        .mutation_journal_stats_for_testing(&tenant_id)
                        .expect("mutation journal stats should load")
                }
            },
            |stats| stats.durable_head.0 >= 2 && stats.applied_head.0 >= 2,
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed writes via journal catch-up",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    engine
                        .query_documents_async(tenant_id, query_for("tasks"))
                        .await
                        .expect("query should succeed")
                }
            },
            |documents| {
                documents.iter().any(|document| {
                    document
                        .fields
                        .get("title")
                        .and_then(|value| value.as_str())
                        == Some("Recovered")
                })
            },
        )
        .await;

        tokio::time::timeout(Duration::from_secs(2), engine.quiesce())
            .await
            .expect("engine should quiesce after reconnect test");
    })
    .await;
}

/// PPSC2-C provider-lane evidence: on a provider backend every drained
/// journal batch is one durable network round trip, so the adaptive cap
/// directly sets ops-per-round-trip. A paused concurrent burst above the
/// base cap must commit in a single append (ops/RTT > 32), while an idle
/// arrival keeps the one-op/one-round-trip baseline.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial(postgres_provider)]
async fn postgres_adaptive_batch_commits_a_burst_in_one_durable_round_trip() {
    with_postgres_engine_config(|engine_config, _provider_config| async move {
        expect_external_provider_future_within(
            "postgres adaptive burst should commit promptly",
            Duration::from_secs(90),
            Duration::from_secs(360),
            async {
                const BURST: usize = 96;
                let tenant_id = TenantId::new("pg-adaptive-batch").expect("tenant id should build");
                let engine = Arc::new(
                    Engine::new_with_persistence_config(engine_config)
                        .await
                        .expect("postgres-backed engine should create"),
                );
                engine
                    .create_tenant_async(tenant_id.clone())
                    .await
                    .expect("tenant should create");
                engine
                    .set_mutation_admission_codel_for_testing(
                        &tenant_id,
                        Duration::from_secs(60),
                        Duration::from_secs(60),
                    )
                    .expect("the burst should not be shed by CoDel");

                let before = engine
                    .tenant_engine_diagnostics(&tenant_id)
                    .expect("phase metrics before burst should load")
                    .commit_phases;
                let pause = engine
                    .mutation_journal_pause_handle_for_testing(&tenant_id)
                    .expect("journal pause handle should load");
                pause.arm();

                let mut inserts = Vec::with_capacity(BURST);
                inserts.push(tokio::spawn({
                    let engine = Arc::clone(&engine);
                    let tenant_id = tenant_id.clone();
                    async move {
                        engine
                            .insert_document_async(
                                tenant_id,
                                tasks_table(),
                                serde_json::Map::from_iter([(
                                    "title".to_string(),
                                    json!("burst-0"),
                                )]),
                            )
                            .await
                    }
                }));
                let entered = tokio::task::spawn_blocking({
                    let pause = pause.clone();
                    move || pause.wait_until_entered(Duration::from_secs(30))
                })
                .await
                .expect("pause wait task should join");
                assert!(
                    entered,
                    "journal worker should pause with the first burst mutation admitted"
                );

                for index in 1..BURST {
                    inserts.push(tokio::spawn({
                        let engine = Arc::clone(&engine);
                        let tenant_id = tenant_id.clone();
                        async move {
                            engine
                                .insert_document_async(
                                    tenant_id,
                                    tasks_table(),
                                    serde_json::Map::from_iter([(
                                        "title".to_string(),
                                        json!(format!("burst-{index}")),
                                    )]),
                                )
                                .await
                        }
                    }));
                }
                let backlog_deadline = std::time::Instant::now() + Duration::from_secs(30);
                loop {
                    let stats = engine
                        .mutation_admission_stats_for_testing(&tenant_id)
                        .expect("admission stats should load");
                    if stats.queue_depth == BURST - 1 {
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < backlog_deadline,
                        "the rest of the burst should queue behind the paused drainer \
                         (saw depth {})",
                        stats.queue_depth
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                pause.release();
                for insert in inserts {
                    insert
                        .await
                        .expect("burst task should join")
                        .expect("burst mutation should succeed");
                }

                let after_burst = engine
                    .tenant_engine_diagnostics(&tenant_id)
                    .expect("phase metrics after burst should load")
                    .commit_phases;
                let burst_ops = after_burst
                    .journal_batch_size_sum
                    .saturating_sub(before.journal_batch_size_sum);
                let burst_round_trips = after_burst
                    .journal_batch_count
                    .saturating_sub(before.journal_batch_count);
                assert_eq!(burst_ops, BURST as u64);
                assert_eq!(
                    burst_round_trips, 1,
                    "the provider backlog should commit in one durable round trip"
                );
                assert!(
                    burst_ops / burst_round_trips > 32,
                    "ops-per-round-trip should scale past the fixed base cap"
                );

                engine
                    .insert_document_async(
                        tenant_id.clone(),
                        tasks_table(),
                        serde_json::Map::from_iter([("title".to_string(), json!("idle"))]),
                    )
                    .await
                    .expect("idle mutation should succeed");
                let after_idle = engine
                    .tenant_engine_diagnostics(&tenant_id)
                    .expect("phase metrics after idle mutation should load")
                    .commit_phases;
                assert_eq!(
                    after_idle
                        .journal_batch_count
                        .saturating_sub(after_burst.journal_batch_count),
                    1
                );
                assert_eq!(
                    after_idle
                        .journal_batch_size_sum
                        .saturating_sub(after_burst.journal_batch_size_sum),
                    1,
                    "an idle arrival should keep the one-op round-trip baseline"
                );

                tokio::time::timeout(Duration::from_secs(5), engine.quiesce())
                    .await
                    .expect("engine should quiesce after the adaptive burst");
            },
        )
        .await;
    })
    .await;
}
