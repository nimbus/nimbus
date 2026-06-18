use super::support::*;

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_manages_tenant_registry_and_namespaces() {
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
            created_alpha.namespace,
            provider
                .tenant_namespace(&alpha)
                .expect("tenant namespace should derive")
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
        assert_eq!(reopened.namespace, created_alpha.namespace);

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
            vec![beta.clone()]
        );

        let recreated_alpha = provider
            .create_tenant(&alpha)
            .await
            .expect("tenant should recreate after delete");
        assert_eq!(
            recreated_alpha.namespace,
            provider
                .tenant_namespace(&alpha)
                .expect("tenant namespace should derive")
        );
        assert_eq!(
            provider.list_tenants().await.expect("tenants should list"),
            vec![alpha, beta]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = LibsqlReplicaProvider::connect(config)
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
                .namespace,
            created.namespace
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn libsql_opened_tenant_materializes_local_sqlite_snapshot() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("opened").expect("tenant id should build");
        let registration = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");
        let table = TableName::new("tasks").expect("table name should build");
        let table_schema = TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            }],
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            }],
            access_policy: None,
        };
        let document_id = DocumentId::new();
        seed_remote_namespace(
            &config,
            &registration.namespace,
            &table_schema,
            document_id.clone(),
            serde_json::json!({
                "rank": 5,
                "title": "from-primary"
            }),
        )
        .await;

        let refreshed_path = provider
            .refresh_tenant_snapshot(&tenant)
            .await
            .expect("tenant snapshot should refresh");
        assert!(
            refreshed_path.exists(),
            "refreshed replica path should exist"
        );

        let opened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("opened tenant should sync and open")
            .expect("tenant should exist");
        assert_eq!(opened.tenant_id(), &tenant);
        assert_eq!(opened.namespace(), registration.namespace);
        assert_eq!(opened.primary_url(), config.primary_url);
        assert_eq!(opened.replica_path(), refreshed_path.as_path());
        assert_eq!(
            opened
                .store
                .read_snapshot()
                .expect("snapshot should open")
                .journal_mode()
                .expect("journal mode should read"),
            "wal"
        );

        let table_for_read = table.clone();
        let indexed = opened
            .read_storage
            .execute(move |store| {
                let snapshot = store.read_snapshot()?;
                let mut check_cancel = || Ok(());
                snapshot.index_scan_eq_cancellable(
                    &table_for_read,
                    "by_rank",
                    &serde_json::json!(5),
                    &mut check_cancel,
                )
            })
            .await
            .expect("async indexed read should succeed");
        assert_eq!(indexed.len(), 1);
        assert_eq!(indexed[0].id, document_id);
        assert_eq!(
            indexed[0].fields.get("title").expect("field should exist"),
            &serde_json::json!("from-primary")
        );
    })
    .await;
}
