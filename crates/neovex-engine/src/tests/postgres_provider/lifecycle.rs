use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_supports_async_tenant_lifecycle_and_empty_read_paths() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-tenant").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert_eq!(
            service
                .list_tenants_async()
                .await
                .expect("tenant list should load"),
            vec![tenant_id.clone()]
        );
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant existence should verify");
        assert_eq!(
            service
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should read"),
            SequenceNumber(0)
        );
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("empty query should succeed")
                .is_empty()
        );
        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(0));
        assert_eq!(bootstrap.resume_after, SequenceNumber(0));
        assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
        assert!(bootstrap.snapshot.documents.is_empty());

        service.quiesce().await;
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
        );
        assert_eq!(
            reopened
                .list_tenants_async()
                .await
                .expect("reopened tenant list should load"),
            vec![tenant_id.clone()]
        );
        reopened
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should lazy load after reopen");
        assert!(
            reopened
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("reopened empty query should succeed")
                .is_empty()
        );
        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn typed_postgres_config_reopens_multiple_tenants_for_concurrent_mixed_ops() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        let mut seeded = Vec::new();
        for index in 0..4 {
            let tenant_id =
                TenantId::new(format!("pg-reopen-mixed-{index}")).expect("tenant id should build");
            service
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            service
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema should persist");
            let document_id = service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("seed-{index}")),
                    )]),
                )
                .await
                .expect("seed insert should succeed");
            seeded.push((tenant_id, document_id));
        }

        service.quiesce().await;
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
        );

        tokio::time::timeout(Duration::from_secs(10), async {
            let mut handles = Vec::new();
            for (index, (tenant_id, document_id)) in seeded.into_iter().enumerate() {
                let service = reopened.clone();
                handles.push(tokio::spawn(async move {
                    let document = service
                        .get_document_async(tenant_id.clone(), tasks_table(), document_id)
                        .await
                        .expect("reopened point read should succeed");
                    let expected_title = format!("seed-{index}");
                    assert_eq!(
                        document
                            .fields
                            .get("title")
                            .and_then(|value| value.as_str()),
                        Some(expected_title.as_str())
                    );

                    let documents = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await
                        .expect("reopened query should succeed");
                    assert_eq!(documents.len(), 1);

                    let inserted_id = service
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(format!("reopened-{index}")),
                            )]),
                        )
                        .await
                        .expect("reopened insert should succeed");
                    service
                        .update_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            inserted_id,
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(format!("updated-{index}")),
                            )]),
                        )
                        .await
                        .expect("reopened update should succeed");
                }));
            }

            for handle in handles {
                handle.await.expect("task should join");
            }
        })
        .await
        .expect("reopened concurrent mixed ops should finish promptly");

        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(postgres_provider)]
async fn postgres_tenant_delete_recreate_cleans_schema_and_runtime_state() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-tenant-delete").expect("tenant id should build");
        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("provider should connect");
        let schema_name = provider
            .tenant_schema_name(&tenant_id)
            .expect("schema name should derive");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert!(matches!(
            service.create_tenant_async(tenant_id.clone()).await,
            Err(Error::AlreadyExists(_))
        ));
        assert!(
            postgres_schema_exists(&provider_config, &schema_name)
                .await
                .expect("schema existence should load"),
            "tenant schema should exist after creation"
        );

        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should load");
        assert!(
            service.loaded_tenant_ids().contains(&tenant_id),
            "tenant should be loaded before deletion"
        );

        service
            .delete_tenant_async(tenant_id.clone())
            .await
            .expect("tenant delete should succeed");
        assert!(
            !service.loaded_tenant_ids().contains(&tenant_id),
            "tenant should evict from the loaded registry after deletion"
        );
        assert!(
            !service
                .list_tenants_async()
                .await
                .expect("tenant list should load")
                .contains(&tenant_id),
            "tenant should disappear from provider-owned registry after deletion"
        );
        assert!(
            !postgres_schema_exists(&provider_config, &schema_name)
                .await
                .expect("schema existence should reload"),
            "tenant schema should be removed after deletion"
        );
        assert!(matches!(
            service.delete_tenant_async(tenant_id.clone()).await,
            Err(Error::TenantNotFound(_))
        ));

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should recreate cleanly");
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("recreated tenant should load");
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("recreated tenant query should succeed")
                .is_empty(),
            "recreated tenant should start empty after schema cleanup"
        );

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after tenant lifecycle test");
    })
    .await;
}
