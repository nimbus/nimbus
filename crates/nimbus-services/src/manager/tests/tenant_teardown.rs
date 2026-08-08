use super::*;

fn project_service_for_retirement(
    manager: &ServiceManager,
    backend: &StubSandboxBackend,
    tenant_id: &TenantId,
    service_name: &str,
) -> SandboxHandle {
    let definition = manager
        .service_definition_for_tenant(tenant_id, service_name)
        .expect("service definition should exist");
    let mut handle = backend.sandbox_handle(tenant_id, service_name, SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, definition.generation, 0);
    manager
        .project_service_definition_execution_observation(
            tenant_id,
            service_name,
            definition.generation,
            &definition.resource_version,
            &execution,
            handle.clone(),
        )
        .expect("service observation should project");
    handle
}

fn reserve_and_project_standalone_for_retirement(
    manager: &ServiceManager,
    backend: &StubSandboxBackend,
    tenant_id: &TenantId,
) -> (crate::SandboxResourceSource, SandboxHandle) {
    let source = reserve_standalone_source(
        manager,
        tenant_id,
        "stable-worker",
        "worker",
        standalone_resource_spec(tenant_id, "task"),
        BTreeMap::new(),
    );
    let mut handle = backend.sandbox_handle(tenant_id, "task", SandboxStatus::Ready);
    let execution = execution_reference_for_handle(&mut handle, source.generation, 0);
    manager
        .project_sandbox_resource_execution_observation(
            tenant_id,
            &source.id,
            source.generation,
            &source.resource_version,
            &execution,
            handle.clone(),
        )
        .expect("standalone observation should project");
    (source, handle)
}

#[tokio::test]
async fn tenant_retirement_stops_observed_sandboxes_and_clears_tenant_resources() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    );

    project_service_for_retirement(&manager, &backend, &tenant_id, "db");
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("dynamic built-in definition should be recorded");
    let (standalone, _) =
        reserve_and_project_standalone_for_retirement(&manager, &backend, &tenant_id);
    manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Sandbox {
                id: standalone.id.clone(),
            },
            vec!["stdio".to_owned()],
            Some(60_000),
        )
        .await
        .expect("standalone sandbox session should open");
    assert!(manager.snapshot_for_tenant(&tenant_id).contains_key("db"));
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "browser")
            .is_some()
    );
    assert_eq!(
        manager
            .list_sandbox_resource_snapshots_for_tenant(&tenant_id)
            .len(),
        1
    );
    assert_eq!(manager.list_sessions_for_tenant(&tenant_id).len(), 1);

    TenantServiceRetirement::retire_tenant_async(&manager, &tenant_id)
        .await
        .expect("tenant retirement should stop tracked resources");

    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        2,
        "tenant retirement should stop service-backed and standalone sandboxes"
    );
    assert_eq!(
        backend.artifact_cleanup_calls.load(Ordering::SeqCst),
        1,
        "tenant retirement should remove tenant-owned sandbox artifact roots"
    );
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "tenant retirement should clear manager snapshots"
    );
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "browser")
            .is_none(),
        "tenant retirement should purge dynamic service definitions"
    );
    assert!(
        manager
            .list_sandbox_resource_snapshots_for_tenant(&tenant_id)
            .is_empty(),
        "tenant retirement should purge standalone sandbox resources"
    );
    assert!(
        manager.list_sessions_for_tenant(&tenant_id).is_empty(),
        "tenant retirement should purge tenant sessions"
    );
}

#[tokio::test]
async fn tenant_retirement_attempts_all_stops_and_clears_successes_before_errors() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(1));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::from([(
                "db".to_owned(),
                image_service_backend("db", "postgres:16"),
            )]),
        }),
        backend.clone(),
    );

    project_service_for_retirement(&manager, &backend, &tenant_id, "db");
    let (standalone, standalone_handle) =
        reserve_and_project_standalone_for_retirement(&manager, &backend, &tenant_id);
    manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Sandbox {
                id: standalone.id.clone(),
            },
            vec!["stdio".to_owned()],
            Some(60_000),
        )
        .await
        .expect("standalone sandbox session should open");
    backend.fail_stop_for(standalone_handle.id.as_str());

    let error = TenantServiceRetirement::retire_tenant_async(&manager, &tenant_id)
        .await
        .expect_err("failed standalone stop should be reported after best-effort retirement");

    assert!(
        error.to_string().contains(standalone_handle.id.as_str())
            && error.to_string().contains("best-effort cleanup"),
        "aggregate retirement error should name the failed sandbox: {error}"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        2,
        "tenant retirement should attempt service-backed and standalone stops"
    );
    assert_eq!(
        backend.artifact_cleanup_calls.load(Ordering::SeqCst),
        1,
        "tenant retirement should still attempt tenant artifact cleanup"
    );
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "successfully stopped service observation should be cleared before returning the aggregate error"
    );
    assert_eq!(
        manager
            .list_sandbox_resource_snapshots_for_tenant(&tenant_id)
            .len(),
        1,
        "failed standalone sandbox source should remain for retry"
    );
    assert_eq!(
        manager.list_sessions_for_tenant(&tenant_id).len(),
        1,
        "session targeting the failed standalone sandbox should remain for retry"
    );
}
