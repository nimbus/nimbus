use super::*;
use crate::{ControlPlaneConfig, LocalEncryptionConfig, TenantProviderConfig};

#[test]
fn tenant_lifecycle_caller_inventory_is_complete() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-engine tests");
    let workspace = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("nimbus-engine should live under the workspace crates directory");
    let output = std::process::Command::new("bash")
        .arg(workspace.join("scripts/verify-tenant-lifecycle-callers.sh"))
        .current_dir(&workspace)
        .output()
        .expect("tenant lifecycle inventory verifier should execute");
    assert!(
        output.status.success(),
        "tenant lifecycle inventory must remain complete\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn embedded_blocking_tenant_creation_supports_redb_and_sqlite() {
    for provider in [EmbeddedProviderKind::Redb, EmbeddedProviderKind::Sqlite] {
        let data_dir = tempdir().expect("temporary data dir should create");
        let engine = Engine::new_with_embedded_provider(data_dir.path(), provider)
            .expect("embedded engine should create");
        let tenant_id = TenantId::new(format!("blocking-{provider:?}").to_lowercase())
            .expect("tenant id should build");

        assert_eq!(
            engine
                .ensure_tenant_ready_blocking(tenant_id.clone())
                .expect("blocking embedded lifecycle should create the tenant"),
            crate::TenantAdmissionOutcome::Created
        );
        assert_eq!(
            engine
                .ensure_tenant_ready_blocking(tenant_id.clone())
                .expect("blocking embedded lifecycle should be idempotent"),
            crate::TenantAdmissionOutcome::Existing
        );
        assert!(
            data_dir
                .path()
                .join(format!(
                    "{}.{}",
                    tenant_id.as_str(),
                    provider.tenant_file_extension()
                ))
                .exists(),
            "blocking lifecycle must persist the provider-specific tenant file"
        );
    }
}

#[tokio::test]
async fn provider_composition_has_no_blocking_tenant_lifecycle() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let engine = Arc::new(
        Engine::new_with_memory_persistence(data_dir.path())
            .expect("memory provider engine should create"),
    );
    let tenant_id = TenantId::new("provider-lifecycle".to_string()).expect("tenant id");

    let error = engine
        .create_tenant(tenant_id.clone())
        .expect_err("provider composition must reject the blocking lifecycle");
    assert!(
        error
            .to_string()
            .contains("embedded-only blocking tenant lifecycle"),
        "unexpected blocking-lifecycle error: {error}"
    );
    let error = engine
        .ensure_tenant_ready_blocking(tenant_id.clone())
        .expect_err("provider composition must reject missing blocking admission");
    assert!(
        error
            .to_string()
            .contains("embedded-only blocking tenant lifecycle"),
        "unexpected blocking-admission error: {error}"
    );
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("async provider lifecycle should create the tenant");
    engine
        .ensure_tenant_exists_async(tenant_id)
        .await
        .expect("async lifecycle must register the runtime");
    assert_eq!(
        engine
            .ensure_tenant_ready_blocking(
                TenantId::new("provider-lifecycle").expect("tenant id should build")
            )
            .expect("blocking command core should accept the pre-admitted provider runtime"),
        crate::TenantAdmissionOutcome::Existing
    );
}

#[tokio::test]
async fn tenant_creation_async_contract_matches_memory_redb_and_sqlite() {
    let memory_dir = tempdir().expect("memory data dir");
    let redb_dir = tempdir().expect("redb data dir");
    let sqlite_dir = tempdir().expect("sqlite data dir");
    for (name, engine) in [
        (
            "memory",
            Arc::new(
                Engine::new_with_memory_persistence(memory_dir.path())
                    .expect("memory provider engine"),
            ),
        ),
        (
            "redb",
            Arc::new(
                Engine::new_with_embedded_provider(redb_dir.path(), EmbeddedProviderKind::Redb)
                    .expect("redb engine"),
            ),
        ),
        (
            "sqlite",
            Arc::new(
                Engine::new_with_embedded_provider(sqlite_dir.path(), EmbeddedProviderKind::Sqlite)
                    .expect("sqlite engine"),
            ),
        ),
    ] {
        let tenant_id = TenantId::new(format!("async-contract-{name}")).expect("tenant id");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("first async create should succeed");
        assert!(matches!(
            engine.create_tenant_async(tenant_id.clone()).await,
            Err(Error::AlreadyExists(_))
        ));
        engine
            .ensure_tenant_exists_async(tenant_id)
            .await
            .expect("created runtime should remain registered");
        engine.quiesce().await;
    }
}

#[tokio::test]
async fn tenant_admission_replays_after_create_cancellation() {
    let data_dir = tempdir().expect("memory data dir");
    let engine = Arc::new(
        Engine::new_with_memory_persistence(data_dir.path()).expect("memory provider engine"),
    );
    let tenant_id = TenantId::new("cancelled-admission").expect("tenant id");
    let pause = engine.pause_tenant_creation_after_provider_for_testing(tenant_id.clone());
    let create_engine = engine.clone();
    let create_tenant = tenant_id.clone();
    let create =
        tokio::spawn(async move { create_engine.create_tenant_async(create_tenant).await });

    pause.wait_until_entered().await;
    create.abort();
    assert!(
        create
            .await
            .expect_err("paused create should be cancelled")
            .is_cancelled()
    );
    pause.release();
    assert!(
        engine
            .list_tenants_async()
            .await
            .expect("durable tenant list should load")
            .contains(&tenant_id),
        "provider creation must be durable before the cancellation point"
    );
    assert!(
        !engine.loaded_tenant_ids().contains(&tenant_id),
        "cancelled creation must not publish a partial runtime"
    );

    assert_eq!(
        engine
            .ensure_tenant_ready_async(tenant_id.clone())
            .await
            .expect("replay should open the durable tenant"),
        crate::TenantAdmissionOutcome::Existing
    );
    assert!(
        engine.loaded_tenant_ids().contains(&tenant_id),
        "admission must register the complete runtime before returning"
    );
}

#[tokio::test]
async fn tenant_admission_preserves_provider_error_and_retries_cleanly() {
    use nimbus_storage::{FaultOccurrence, ScriptedFaultInjector};

    let data_dir = tempdir().expect("memory data dir");
    let faults = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
        point: FaultPoint::TenantCreateBeforeRegistration,
        visit: 1,
    }]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(nimbus_core::SystemWallClock),
            faults,
            Arc::new(nimbus_core::SystemIdSource),
        )
        .expect("memory provider engine"),
    );
    let tenant_id = TenantId::new("provider-error-admission").expect("tenant id");

    let error = engine
        .ensure_tenant_ready_async(tenant_id.clone())
        .await
        .expect_err("the provider error must remain visible");
    assert!(
        error
            .to_string()
            .contains("tenant_create_before_registration"),
        "unexpected provider error: {error}"
    );
    assert!(
        !engine
            .list_tenants_async()
            .await
            .expect("tenant list should load")
            .contains(&tenant_id),
        "a pre-registration provider error must not leave durable tenant state"
    );
    assert!(!engine.loaded_tenant_ids().contains(&tenant_id));

    assert_eq!(
        engine
            .ensure_tenant_ready_async(tenant_id.clone())
            .await
            .expect("retry should create cleanly"),
        crate::TenantAdmissionOutcome::Created
    );
    engine
        .ensure_tenant_exists_async(tenant_id)
        .await
        .expect("retry must register the runtime");
}

#[tokio::test]
async fn concurrent_tenant_admission_converges_on_one_runtime() {
    let data_dir = tempdir().expect("memory data dir");
    let engine = Arc::new(
        Engine::new_with_memory_persistence(data_dir.path()).expect("memory provider engine"),
    );
    let tenant_id = TenantId::new("concurrent-admission").expect("tenant id");
    let (left, right) = tokio::join!(
        engine.ensure_tenant_ready_async(tenant_id.clone()),
        engine.ensure_tenant_ready_async(tenant_id.clone()),
    );
    let mut outcomes = [
        left.expect("left admission should succeed"),
        right.expect("right admission should succeed"),
    ];
    outcomes.sort_by_key(|outcome| match outcome {
        crate::TenantAdmissionOutcome::Created => 0,
        crate::TenantAdmissionOutcome::Existing => 1,
    });
    assert_eq!(
        outcomes,
        [
            crate::TenantAdmissionOutcome::Created,
            crate::TenantAdmissionOutcome::Existing,
        ]
    );
    assert_eq!(
        engine
            .loaded_tenant_ids()
            .into_iter()
            .filter(|loaded| loaded == &tenant_id)
            .count(),
        1,
        "concurrent admission must publish one runtime"
    );
}

#[test]
fn redb_provider_constructor_preserves_existing_tenant_filename() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let engine = Engine::new_with_embedded_provider(data_dir.path(), EmbeddedProviderKind::Redb)
        .expect("redb-backed engine should create");
    let tenant_id = TenantId::new("demo".to_string()).expect("tenant id should build");

    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    assert!(
        data_dir.path().join("demo.redb").exists(),
        "the retained redb embedded provider must preserve the redb tenant filename"
    );
}

#[tokio::test]
async fn default_engine_constructor_uses_sqlite_tenant_files_and_roundtrips_engine_paths() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo".to_string()).expect("tenant id should build");

    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    assert!(
        data_dir.path().join("demo.sqlite3").exists(),
        "the default embedded provider should persist tenant data under the sqlite3 extension"
    );

    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("sqlite"))]),
        )
        .await
        .expect("sqlite-backed insert should succeed");
    engine.quiesce().await;
    drop(engine);

    let reopened = Arc::new(Engine::new(data_dir.path()).expect("engine should reopen"));
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
async fn default_embedded_provider_works_with_engine_fixture_harness() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("fixture", |engine, tenant_id| {
        engine.create_tenant(tenant_id)
    });

    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("fixture"))]),
        )
        .await
        .expect("fixture-backed sqlite insert should succeed");
    let document = engine
        .get_document_async(tenant_id, tasks_table(), document_id)
        .await
        .expect("fixture-backed default read should succeed");
    assert_eq!(document.fields.get("title"), Some(&json!("fixture")));
}

#[tokio::test]
async fn typed_persistence_config_constructor_preserves_default_sqlite_behavior() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let engine = Arc::new(
        Engine::new_with_persistence_config(EnginePersistenceConfig::embedded_default(
            data_dir.path(),
        ))
        .await
        .expect("typed embedded sqlite engine should create"),
    );
    let tenant_id = TenantId::new("typed-sqlite".to_string()).expect("tenant id should build");

    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        data_dir.path().join("typed-sqlite.sqlite3").exists(),
        "typed embedded sqlite config should preserve the sqlite tenant extension"
    );
    assert_eq!(
        engine
            .list_tenants_async()
            .await
            .expect("async tenant list should load"),
        vec![tenant_id]
    );
}

#[tokio::test]
async fn typed_persistence_config_constructor_supports_explicit_redb_embedded_provider() {
    let data_dir = tempdir().expect("temporary data dir should create");
    let engine = Arc::new(
        Engine::new_with_persistence_config(EnginePersistenceConfig::embedded(
            data_dir.path(),
            EmbeddedProviderKind::Redb,
        ))
        .await
        .expect("typed embedded redb engine should create"),
    );
    let tenant_id = TenantId::new("typed-redb".to_string()).expect("tenant id should build");

    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        data_dir.path().join("typed-redb.redb").exists(),
        "typed embedded redb config should preserve the redb tenant extension"
    );
    assert_eq!(
        engine
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
    let config = EnginePersistenceConfig {
        tenant_provider: TenantProviderConfig::embedded(
            tenant_dir.path(),
            EmbeddedProviderKind::Sqlite,
        ),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let engine = Arc::new(
        Engine::new_with_persistence_config(config.clone())
            .await
            .expect("typed embedded sqlite engine with split control plane should create"),
    );
    let tenant_id =
        TenantId::new("split-control-plane".to_string()).expect("tenant id should build");

    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    assert!(
        tenant_dir
            .path()
            .join("split-control-plane.sqlite3")
            .exists(),
        "tenant data should remain in the configured sqlite tenant directory"
    );

    engine
        .record_monthly_active_user("alice")
        .expect("usage write should succeed");
    assert!(
        control_dir
            .path()
            .join(EmbeddedProviderKind::Redb.control_database_filename())
            .exists(),
        "the control-plane database should be created in the configured control directory once usage state is touched"
    );
    assert_eq!(
        engine
            .current_monthly_active_users()
            .expect("usage snapshot should load")
            .monthly_active_users,
        1
    );

    engine.quiesce().await;
    drop(engine);

    let reopened = Arc::new(
        Engine::new_with_persistence_config(config)
            .await
            .expect("engine should reopen with split control-plane config"),
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
