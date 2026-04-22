use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-mutations").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
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

        let inserted_id = service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("First"))]),
            )
            .await
            .expect("insert should succeed");
        service
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                inserted_id,
                serde_json::Map::from_iter([("title".to_string(), json!("Renamed"))]),
            )
            .await
            .expect("update should succeed");

        let scheduled_job_id = service
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 5_000,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
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
            service
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        let claimed = service
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        service
            .record_scheduled_job_result_async(
                tenant_id.clone(),
                neovex_core::ScheduledJobResult {
                    id: scheduled_job_id,
                    run_at: claimed[0].run_at,
                    finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                    mutation: claimed[0].mutation.clone(),
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .await
            .expect("scheduled result should persist");
        service
            .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id)
            .await
            .expect("scheduled completion should persist");
        assert_eq!(
            service
                .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id)
                .await
                .expect("scheduled result should load")
                .outcome,
            ScheduledJobOutcome::Completed
        );

        let documents = service
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

        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(2));
        assert_eq!(bootstrap.resume_after, SequenceNumber(2));

        service.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_notifications_refresh_loaded_runtime_schema_and_journal_state() {
    with_shared_postgres_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-journal").expect("tenant id should build");
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

            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            service_b
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("second service should load tenant");
            assert_eq!(
                service_b
                    .get_schema_async(tenant_id.clone())
                    .await
                    .expect("empty schema should load"),
                Schema::default()
            );

            service_a
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema write should succeed");
            service_a
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
                    let service = service_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
                            .get_schema_async(tenant_id)
                            .await
                            .expect("schema should load")
                    }
                },
                |schema| schema.get_table(&tasks_table()).is_some(),
            )
            .await;
            wait_for_mutation_journal_stats(
                &service_b,
                &tenant_id,
                "postgres notification should catch up journal heads",
                |stats| {
                    stats.durable_head == SequenceNumber(1)
                        && stats.applied_head == SequenceNumber(1)
                },
            )
            .await;

            let documents = service_b
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

            tokio::time::timeout(Duration::from_secs(2), service_a.quiesce())
                .await
                .expect("first service should quiesce after reconnect test");
            tokio::time::timeout(Duration::from_secs(2), service_b.quiesce())
                .await
                .expect("second service should quiesce after reconnect test");
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_listener_reconnect_recovers_missed_schema_and_journal_hints() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-notify-reconnect").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("service should load tenant");
        assert_eq!(
            service
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
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    service
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
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    service
                        .mutation_journal_stats_for_testing(&tenant_id)
                        .expect("mutation journal stats should load")
                }
            },
            |stats| {
                stats.durable_head == SequenceNumber(1) && stats.applied_head == SequenceNumber(1)
            },
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed writes via journal catch-up",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let service = service.clone();
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
                        == Some("Recovered")
                })
            },
        )
        .await;

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after reconnect test");
    })
    .await;
}
