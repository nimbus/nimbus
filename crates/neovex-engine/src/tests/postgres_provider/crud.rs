use super::support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_keeps_sequence_heads_in_sync_across_repeated_direct_crud() {
    // This lane validates correctness, not a tight performance SLO. Under the
    // full workspace test load in CI, external-provider tests can run much
    // slower than the focused local case, so keep enough headroom to catch
    // real hangs without flaking on runner contention.
    expect_external_provider_future_within(
        "postgres repeated direct CRUD should finish promptly",
        Duration::from_secs(60),
        Duration::from_secs(180),
        async {
            with_postgres_service_config(|service_config, _provider_config| async move {
                let tenant_id = TenantId::new("pg-repeated-crud").expect("tenant id should build");
                let service = Arc::new(
                    Service::new_with_persistence_config(service_config)
                        .await
                        .expect("postgres-backed service should create"),
                );

                service
                    .create_tenant_async(tenant_id.clone())
                    .await
                    .expect("tenant should create");

                // Keep enough direct CRUD churn to validate sequence/head
                // correctness, but do not turn this external-provider lane
                // into a throughput benchmark under shared CI runners.
                const CRUD_ROUNDS: usize = 48;
                for round in 0..CRUD_ROUNDS {
                    let document_id = service
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([
                                ("title".to_string(), json!(format!("round-{round}"))),
                                ("rank".to_string(), json!(round)),
                            ]),
                        )
                        .await
                        .expect("insert should succeed");
                    service
                        .update_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            document_id.clone(),
                            serde_json::Map::from_iter([(
                                "rank".to_string(),
                                json!(round + CRUD_ROUNDS),
                            )]),
                        )
                        .await
                        .expect("update should succeed");
                    service
                        .delete_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            document_id.clone(),
                        )
                        .await
                        .expect("delete should succeed");
                }

                assert_eq!(
                    service
                        .latest_sequence_async(tenant_id.clone())
                        .await
                        .expect("latest sequence should track every direct mutation"),
                    SequenceNumber((CRUD_ROUNDS * 3) as u64)
                );
                assert!(
                    service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await
                        .expect("query should succeed after repeated CRUD")
                        .is_empty(),
                    "repeated direct CRUD should leave no remaining documents"
                );

                tokio::time::timeout(Duration::from_secs(2), service.quiesce())
                    .await
                    .expect("service should quiesce before Postgres fixture cleanup");
                drop(service);
            })
            .await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_transient_pool_backend_termination_recovers_subsequent_mixed_ops() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-pool-recover").expect("tenant id should build");
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
        for index in 0..8 {
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("Seed {index}")),
                    )]),
                )
                .await
                .expect("seed insert should succeed");
        }

        let original_pool_pids = wait_for_value(
            "postgres pool should expose live backends before termination",
            Duration::from_secs(2),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                async move {
                    list_postgres_pool_backend_pids(&provider_config)
                        .await
                        .expect("pool pid list should load")
                }
            },
            |pids| !pids.is_empty(),
        )
        .await
        .into_iter()
        .collect::<BTreeSet<_>>();
        terminate_postgres_pool_backends(&provider_config)
            .await
            .expect("pool backend termination should succeed");

        wait_for_value(
            "postgres pool should recreate terminated backends",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                let original_pool_pids = original_pool_pids.clone();
                async move {
                    let current = list_postgres_pool_backend_pids(&provider_config)
                        .await
                        .expect("pool pid list should load");
                    current
                        .into_iter()
                        .any(|pid| !original_pool_pids.contains(&pid))
                }
            },
            |restored| *restored,
        )
        .await;

        let recovered_title = format!("Recovered {}", unique_suffix());
        wait_for_value(
            "postgres pooled backend termination should recover mixed ops",
            Duration::from_secs(4),
            Duration::from_millis(50),
            || {
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                let recovered_title = recovered_title.clone();
                async move {
                    let existing = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await;
                    if let Ok(documents) = existing
                        && documents.iter().any(|document| {
                            document
                                .fields
                                .get("title")
                                .and_then(|value| value.as_str())
                                == Some(recovered_title.as_str())
                        })
                    {
                        return true;
                    }

                    let insert = service
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(recovered_title.clone()),
                            )]),
                        )
                        .await;
                    let bootstrap = service
                        .export_durable_journal_bootstrap_async(tenant_id.clone())
                        .await;
                    let query = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await;
                    match (insert, bootstrap, query) {
                        (Ok(_), Ok(bootstrap), Ok(documents)) => {
                            bootstrap.resume_after.0 >= 9
                                && documents.iter().any(|document| {
                                    document
                                        .fields
                                        .get("title")
                                        .and_then(|value| value.as_str())
                                        == Some(recovered_title.as_str())
                                })
                        }
                        _ => false,
                    }
                }
            },
            |recovered| *recovered,
        )
        .await;

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after pooled-backend recovery test");
    })
    .await;
}
