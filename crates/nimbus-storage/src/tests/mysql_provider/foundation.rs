use super::support::*;

#[tokio::test(flavor = "multi_thread")]
async fn mysql_provider_manages_tenant_registry_and_databases() {
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
            created_alpha.database_name,
            provider
                .tenant_database_name(&alpha)
                .expect("tenant database should derive")
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
        assert_eq!(reopened.database_name, created_alpha.database_name);

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
async fn mysql_provider_reloads_registry_after_reconnect() {
    with_test_provider(|provider, config| async move {
        let tenant = TenantId::new("reload").expect("tenant id should build");
        let created = provider
            .create_tenant(&tenant)
            .await
            .expect("tenant should create");

        let reopened = MySqlProvider::connect(config)
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
                .database_name,
            created.database_name
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_opened_tenant_exposes_store_identity_and_read_storage() {
    with_test_provider(|provider, _config| async move {
        let tenant = TenantId::new("opened").expect("tenant id should build");
        let opened = provider
            .create_opened_tenant(&tenant)
            .await
            .expect("tenant should create and open");

        assert_eq!(opened.store.tenant_id(), &tenant);
        assert_eq!(
            opened.store.database_name(),
            provider
                .tenant_database_name(&tenant)
                .expect("tenant database should derive")
        );
        assert_eq!(
            opened
                .read_storage
                .execute(|store| Ok((store.tenant_id().clone(), store.database_name().to_string())))
                .await
                .expect("read storage should execute"),
            (
                tenant.clone(),
                provider
                    .tenant_database_name(&tenant)
                    .expect("tenant database should derive")
            )
        );

        let reopened = provider
            .open_existing_opened_tenant(&tenant)
            .await
            .expect("tenant reopen should succeed")
            .expect("tenant should exist");
        assert_eq!(reopened.store.tenant_id(), &tenant);
        assert_eq!(reopened.store.database_name(), opened.store.database_name());
    })
    .await;
}
