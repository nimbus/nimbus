use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_notifications_load_unloaded_tenants_with_scheduled_work() {
    with_shared_postgres_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-scheduler").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first postgres-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second postgres-backed service should create"),
            );

            service_b
                .load_tenants_with_scheduled_work_async()
                .await
                .expect("initial scheduled-work preload should succeed");
            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");

            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let scheduler_handle =
                tokio::spawn(crate::run_scheduler(service_b.clone(), shutdown_rx));
            service_a
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
                    let service = service_a.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
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
                "postgres notification should load the scheduled tenant into the second service",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    async move { service.loaded_tenant_ids() }
                },
                |tenant_ids| tenant_ids.contains(&tenant_id),
            )
            .await;

            let _ = shutdown_tx.send(true);
            scheduler_handle.await.expect("scheduler should shut down");
            service_a.quiesce().await;
            service_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_restart_recovers_due_scheduler_work_after_reopen() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-restart-scheduler").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");
        let scheduled_job_id = service
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
            service
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce before restart");
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
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
