use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_scheduler_writes_are_atomically_fenced() {
    with_shared_postgres_engine_configs(
        |engine_config_a, engine_config_b, provider_config| async move {
            let tenant_id = TenantId::new("pg-scheduler-fence").expect("tenant id should build");
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
            let tenant_id_for_expiry = tenant_id.clone();
            exercise_provider_scheduler_fence_contract(
                engine_a,
                engine_b,
                tenant_id,
                move || async move {
                    expire_postgres_committer_lease(&provider_config, &tenant_id_for_expiry)
                        .await
                        .expect("postgres scheduler holder lease should expire");
                },
            )
            .await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_notifications_load_unloaded_tenants_with_scheduled_work() {
    with_shared_postgres_engine_configs(
        |engine_config_a, engine_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-scheduler").expect("tenant id should build");
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

            engine_b
                .load_tenants_with_scheduled_work_async()
                .await
                .expect("initial scheduled-work preload should succeed");
            engine_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");

            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let scheduler_handle =
                tokio::spawn(crate::run_scheduler(engine_b.clone(), shutdown_rx));
            engine_a
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: 0,
                        mutation: Mutation::Insert {
                            table: tasks_table(),
                            id: None,
                            fields: serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!("Scheduled externally"),
                            )]),
                        },
                    },
                )
                .await
                .expect("scheduled mutation should persist");

            wait_for_value(
                "postgres notification should load tenant and execute scheduled work",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let engine = engine_a.clone();
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
                            == Some("Scheduled externally")
                    })
                },
            )
            .await;
            wait_for_value(
                "postgres notification should load the scheduled tenant into the second engine",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let engine = engine_b.clone();
                    async move { engine.loaded_tenant_ids() }
                },
                |tenant_ids| tenant_ids.contains(&tenant_id),
            )
            .await;

            let _ = shutdown_tx.send(true);
            scheduler_handle.await.expect("scheduler should shut down");
            engine_a.quiesce().await;
            engine_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_restart_recovers_due_scheduler_work_after_reopen() {
    with_postgres_engine_config(|engine_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-restart-scheduler").expect("tenant id should build");
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config.clone())
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
        let scheduled_job_id = engine
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
                        id: None,
                        fields: serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("Recovered after restart"),
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

        tokio::time::timeout(Duration::from_secs(2), engine.quiesce())
            .await
            .expect("engine should quiesce before restart");
        // Quiesce stops renewal but intentionally leaves the provider-time
        // lease intact. Cross the handoff boundary explicitly so the reopened
        // scheduler acquires a fresh epoch without a wall-clock sleep.
        expire_postgres_committer_lease(&provider_config, &tenant_id)
            .await
            .expect("quiesced scheduler tenant lease should expire for reopen");
        drop(engine);

        let reopened = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("postgres-backed engine should reopen"),
        );
        // External-provider startup recovery preloads scheduled-work tenants
        // and running-job recovery across real Postgres connections. Allow a
        // slightly wider bound here so the default container-backed path stays
        // deterministic under colder connection and statement-cache startup.
        tokio::time::timeout(
            Duration::from_secs(5),
            reopened.recover_scheduled_work_on_startup_async(),
        )
        .await
        .expect("startup scheduled-work recovery should finish promptly after reopen")
        .expect("startup scheduled-work recovery should succeed after reopen");
        tokio::time::timeout(
            Duration::from_secs(15),
            crate::scheduler::tick_async(&reopened),
        )
        .await
        .expect("scheduler tick should finish after restart recovery")
        .expect("scheduler tick should process recovered work after reopen");

        let documents = tokio::time::timeout(
            Duration::from_secs(5),
            reopened.query_documents_async(tenant_id.clone(), query_for("tasks")),
        )
        .await
        .expect("query should finish after restart recovery")
        .expect("query should succeed after restart recovery");
        assert!(documents.iter().any(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                == Some("Recovered after restart")
        }));
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                reopened
                    .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id.clone()),
            )
            .await
            .expect("scheduled job result should finish after restart recovery")
            .expect("scheduled job result should load after restart recovery")
            .outcome,
            ScheduledJobOutcome::Completed
        );
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                reopened.list_scheduled_jobs_async(tenant_id),
            )
            .await
            .expect("scheduled jobs should finish after restart recovery")
            .expect("scheduled jobs should load after restart recovery")
            .len(),
            0
        );
        drop(reopened);
    })
    .await;
}
