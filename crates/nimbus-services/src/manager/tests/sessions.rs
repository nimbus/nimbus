use super::*;

#[tokio::test]
async fn open_session_rejects_not_ready_sandbox_targets() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
    let backend = Arc::new(StubSandboxBackend::new(usize::MAX));
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        backend,
    );
    let sandbox = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_resource_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("standalone sandbox should start in a non-ready state");

    let error = manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Sandbox {
                id: sandbox.id.clone(),
            },
            vec!["stdio".to_owned()],
            Some(60_000),
        )
        .await
        .expect_err("sessions must not attach to a not-ready sandbox");

    assert!(
        error
            .to_string()
            .contains("session open requires a ready sandbox target"),
        "session open should explain ready-state requirement: {error}"
    );
    assert!(
        manager.list_sessions_for_tenant(&tenant_id).is_empty(),
        "rejected not-ready sandbox session must not create a session resource"
    );
}

#[tokio::test]
async fn session_lookup_and_close_are_tenant_scoped() {
    let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
    let other_tenant_id = TenantId::new("tenant-b").expect("tenant id should be valid");
    let manager = ServiceManager::new(
        Arc::new(StubServiceDefinitionCatalog {
            launches: BTreeMap::new(),
        }),
        Arc::new(StubSandboxBackend::new(1)),
    );
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("dynamic browser service definition should create");
    let session = manager
        .open_session_async(
            &tenant_id,
            SessionTarget::Service {
                name: "browser".to_owned(),
            },
            vec!["cdp".to_owned()],
            Some(60_000),
        )
        .await
        .expect("tenant-a service session should open");

    assert!(
        manager.get_session(&other_tenant_id, &session.id).is_none(),
        "wrong-tenant lookup must not expose another tenant's session"
    );
    assert!(
        manager
            .close_session(&other_tenant_id, &session.id, "wrong_tenant")
            .is_none(),
        "wrong-tenant close must not mutate another tenant's session"
    );
    let tenant_session = manager
        .get_session(&tenant_id, &session.id)
        .expect("owning tenant should still see the open session");
    assert_eq!(tenant_session.lifecycle_state, SessionLifecycleState::Open);
    assert_eq!(tenant_session.close_reason, None);

    let closed = manager
        .close_session(&tenant_id, &session.id, "tenant_close")
        .expect("owning tenant should close its session");
    assert_eq!(closed.lifecycle_state, SessionLifecycleState::Closed);
    assert_eq!(closed.close_reason.as_deref(), Some("tenant_close"));
}
