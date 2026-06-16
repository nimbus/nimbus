use super::*;

#[tokio::test]
async fn teardown_tenant_stops_tracked_sandboxes_and_clears_tenant_resources() {
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
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect("service activation should succeed")
        .expect("db binding should exist");
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("dynamic built-in definition should be recorded");
    let standalone = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("standalone sandbox should start");
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
        manager.list_sandbox_resources_for_tenant(&tenant_id).len(),
        1
    );
    assert_eq!(manager.list_sessions_for_tenant(&tenant_id).len(), 1);

    manager
        .teardown_tenant_async(&tenant_id)
        .await
        .expect("tenant teardown should stop tracked resources");

    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        2,
        "tenant teardown should stop service-backed and standalone sandboxes"
    );
    assert_eq!(
        backend.artifact_cleanup_calls.load(Ordering::SeqCst),
        1,
        "tenant teardown should remove tenant-owned sandbox artifact roots"
    );
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "tenant teardown should clear manager snapshots"
    );
    assert!(
        manager
            .service_definition_for_tenant(&tenant_id, "browser")
            .is_none(),
        "tenant teardown should purge dynamic service definitions"
    );
    assert!(
        manager
            .list_sandbox_resources_for_tenant(&tenant_id)
            .is_empty(),
        "tenant teardown should purge standalone sandbox resources"
    );
    assert!(
        manager.list_sessions_for_tenant(&tenant_id).is_empty(),
        "tenant teardown should purge tenant sessions"
    );
}

#[tokio::test]
async fn teardown_tenant_attempts_all_stops_and_clears_successes_before_returning_errors() {
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
    )
    .with_activation_poll_interval(Duration::from_millis(1))
    .with_activation_timeout(Duration::from_secs(1));

    manager
        .ensure_service_binding_async(&tenant_id, "db", HostCallCancellation::default())
        .await
        .expect("service activation should succeed")
        .expect("db binding should exist");
    let standalone = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("standalone sandbox should start");
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
    backend.fail_stop_for(standalone.id.as_str());

    let error = manager
        .teardown_tenant_async(&tenant_id)
        .await
        .expect_err("failed standalone stop should be reported after best-effort teardown");

    assert!(
        error.to_string().contains(standalone.id.as_str())
            && error.to_string().contains("best-effort cleanup"),
        "aggregate teardown error should name the failed sandbox: {error}"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        2,
        "tenant teardown should attempt service-backed and standalone stops"
    );
    assert_eq!(
        backend.artifact_cleanup_calls.load(Ordering::SeqCst),
        1,
        "tenant teardown should still attempt tenant artifact cleanup"
    );
    assert!(
        manager.snapshot_for_tenant(&tenant_id).is_empty(),
        "successfully stopped service handle should be cleared before returning the aggregate error"
    );
    assert_eq!(
        manager.list_sandbox_resources_for_tenant(&tenant_id).len(),
        1,
        "failed standalone sandbox resource should remain for retry"
    );
    assert_eq!(
        manager.list_sessions_for_tenant(&tenant_id).len(),
        1,
        "session targeting the failed standalone sandbox should remain for retry"
    );
}
