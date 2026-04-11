use super::*;
use crate::{ControlPlaneConfig, TenantProviderConfig};

#[test]
fn redb_provider_constructor_preserves_existing_tenant_filename() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let service = Service::new_with_embedded_provider(data_dir.path(), EmbeddedProviderKind::Redb)
        .expect("redb-backed service should create");
    let tenant_id = TenantId::new("demo".to_string()).expect("tenant id should build");

    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    assert!(
        data_dir.path().join("demo.redb").exists(),
        "the retained redb embedded provider must preserve the redb tenant filename"
    );
}

#[tokio::test]
async fn default_service_constructor_uses_sqlite_tenant_files_and_roundtrips_service_paths() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo".to_string()).expect("tenant id should build");

    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    assert!(
        data_dir.path().join("demo.sqlite3").exists(),
        "the default embedded provider should persist tenant data under the sqlite3 extension"
    );

    let document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("sqlite"))]),
        )
        .await
        .expect("sqlite-backed insert should succeed");
    service.quiesce().await;
    drop(service);

    let reopened = Arc::new(Service::new(data_dir.path()).expect("service should reopen"));
    let document = reopened
        .get_document_async(tenant_id.clone(), tasks_table(), document_id)
        .await
        .expect("default-backed lazy load should succeed");
    assert_eq!(document.fields.get("title"), Some(&json!("sqlite")));
    assert_eq!(
        reopened.list_tenants().expect("tenant list should load"),
        vec![tenant_id.clone()]
    );
    assert_eq!(
        reopened
            .list_tenants_async()
            .await
            .expect("async tenant list should load"),
        vec![tenant_id]
    );
}

#[tokio::test]
async fn default_embedded_provider_works_with_service_fixture_harness() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("fixture", |service, tenant_id| {
        service.create_tenant(tenant_id)
    });

    let document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("fixture"))]),
        )
        .await
        .expect("fixture-backed sqlite insert should succeed");
    let document = service
        .get_document_async(tenant_id, tasks_table(), document_id)
        .await
        .expect("fixture-backed default read should succeed");
    assert_eq!(document.fields.get("title"), Some(&json!("fixture")));
}

#[tokio::test]
async fn typed_persistence_config_constructor_preserves_default_sqlite_behavior() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let service = Arc::new(
        Service::new_with_persistence_config(ServicePersistenceConfig::embedded_default(
            data_dir.path(),
        ))
        .await
        .expect("typed embedded sqlite service should create"),
    );
    let tenant_id = TenantId::new("typed-sqlite".to_string()).expect("tenant id should build");

    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        data_dir.path().join("typed-sqlite.sqlite3").exists(),
        "typed embedded sqlite config should preserve the sqlite tenant extension"
    );
    assert_eq!(
        service
            .list_tenants_async()
            .await
            .expect("async tenant list should load"),
        vec![tenant_id]
    );
}

#[tokio::test]
async fn typed_persistence_config_constructor_supports_explicit_redb_embedded_provider() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let service = Arc::new(
        Service::new_with_persistence_config(ServicePersistenceConfig::embedded(
            data_dir.path(),
            EmbeddedProviderKind::Redb,
        ))
        .await
        .expect("typed embedded redb service should create"),
    );
    let tenant_id = TenantId::new("typed-redb".to_string()).expect("tenant id should build");

    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        data_dir.path().join("typed-redb.redb").exists(),
        "typed embedded redb config should preserve the redb tenant extension"
    );
    assert_eq!(
        service
            .list_tenants_async()
            .await
            .expect("async tenant list should load"),
        vec![tenant_id]
    );
}

#[tokio::test]
async fn typed_persistence_config_supports_separate_embedded_control_plane_directory() {
    let tenant_dir = tempdir().expect("tenant data dir should create");
    let control_dir = tempdir().expect("control data dir should create");
    let config = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig::embedded(
            tenant_dir.path(),
            EmbeddedProviderKind::Sqlite,
        ),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir.path()),
    };
    let service = Arc::new(
        Service::new_with_persistence_config(config.clone())
            .await
            .expect("typed embedded sqlite service with split control plane should create"),
    );
    let tenant_id =
        TenantId::new("split-control-plane".to_string()).expect("tenant id should build");

    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        tenant_dir
            .path()
            .join("split-control-plane.sqlite3")
            .exists(),
        "tenant data should remain in the configured sqlite tenant directory"
    );
    assert!(
        control_dir
            .path()
            .join(EmbeddedProviderKind::Redb.control_database_filename())
            .exists(),
        "the explicit control-plane provider should own the redb control database in the control directory"
    );

    service
        .record_monthly_active_user("alice")
        .expect("usage write should succeed");
    assert_eq!(
        service
            .current_monthly_active_users()
            .expect("usage snapshot should load")
            .monthly_active_users,
        1
    );

    service.quiesce().await;
    drop(service);

    let reopened = Arc::new(
        Service::new_with_persistence_config(config)
            .await
            .expect("service should reopen with split control-plane config"),
    );
    assert_eq!(
        reopened
            .current_monthly_active_users()
            .expect("usage snapshot should persist across reopen")
            .monthly_active_users,
        1
    );
    assert_eq!(
        reopened
            .list_tenants_async()
            .await
            .expect("tenant list should still come from tenant persistence"),
        vec![tenant_id]
    );
}
