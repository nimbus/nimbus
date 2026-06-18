use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_manages_tenant_registry_and_schemas() {
    with_test_provider(|provider, _config| async move {
        let alpha = TenantId::new("alpha").expect("tenant id should build");
        let beta = TenantId::new("beta").expect("tenant id should build");

        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            Vec::<TenantId>::new()
        );

        let created_alpha = provider
            .create_tenant(&alpha)
            .await
            .expect("tenant should create");
        assert_eq!(
            created_alpha.schema_name,
            provider
                .tenant_schema_name(&alpha)
                .expect("tenant schema should derive")
        );
        assert!(
            provider
                .tenant_exists(&alpha)
                .await
                .expect("tenant existence should query")
        );

        let duplicate = provider.create_tenant(&alpha).await;
        assert!(matches!(
            duplicate,
            Err(nimbus_core::Error::AlreadyExists(_))
        ));

        provider
            .create_tenant(&beta)
            .await
            .expect("second tenant should create");
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![alpha.clone(), beta.clone()]
        );

        let reopened = provider
            .open_existing_tenant(&alpha)
            .await
            .expect("tenant should open")
            .expect("tenant should exist");
        assert_eq!(reopened.schema_name, created_alpha.schema_name);

        provider
            .delete_tenant(&alpha)
            .await
            .expect("tenant should delete");
        assert!(
            !provider
                .tenant_exists(&alpha)
                .await
                .expect("tenant existence should query")
        );
        assert!(
            provider
                .open_existing_tenant(&alpha)
                .await
                .expect("tenant open should succeed")
                .is_none()
        );
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![beta]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = PostgresProvider::connect(config)
            .await
            .expect("provider should reconnect");
        assert_eq!(
            reopened.list_tenants().await.expect("tenants should list"),
            vec![tenant.clone()]
        );
        assert_eq!(
            reopened
                .open_existing_tenant(&tenant)
                .await
                .expect("tenant should open")
                .expect("tenant should exist")
                .schema_name,
            created.schema_name
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_tenant_store_exposes_empty_read_foundation_after_create() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("foundation").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(
            opened.store.load_schema().expect("schema should load"),
            Schema::default()
        );
        assert_eq!(
            opened
                .store
                .journal_progress()
                .expect("journal progress should load"),
            crate::store::JournalProgress {
                durable_head: SequenceNumber(0),
                applied_head: SequenceNumber(0),
            }
        );
        assert_eq!(
            opened
                .store
                .get(
                    &TableName::new("tasks").expect("table should build"),
                    &nimbus_core::DocumentId::new(),
                )
                .expect("point read should succeed"),
            None
        );

        let bootstrap = opened
            .store
            .export_durable_journal_bootstrap()
            .expect("bootstrap should export");
        assert_eq!(bootstrap.resume_after, SequenceNumber(0));
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(0));
        assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
        assert_eq!(bootstrap.snapshot.schema, Schema::default());
        assert!(bootstrap.snapshot.documents.is_empty());
        assert!(bootstrap.snapshot.scheduled_execution_ids.is_empty());

        let snapshot = opened.store.read_snapshot().expect("snapshot should load");
        assert_eq!(
            snapshot
                .applied_sequence()
                .expect("snapshot applied sequence should load"),
            SequenceNumber(0)
        );
        assert!(
            snapshot
                .scan_table_matching_with_filters_cancellable(
                    &TableName::new("tasks").expect("table should build"),
                    &[],
                    &mut || Ok(()),
                    |_document| Ok(true),
                )
                .expect("snapshot scan should succeed")
                .is_empty()
        );
    })
    .await;
}
