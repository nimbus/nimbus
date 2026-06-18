use super::*;
use crate::manager::session_channels::{
    SessionChannelAuditKind, SessionChannelHalfState, SessionChannelKey,
};

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

#[tokio::test]
async fn session_channel_target_generation_mismatch() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
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
        .expect("service session should open");

    let channel = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned")
        .session_channels
        .get(&SessionChannelKey::new(&session.id, "cdp"))
        .expect("session open should create a channel state")
        .clone();

    let error = channel
        .ensure_target_generation(channel.target_generation + 1)
        .expect_err("stale target generation must reject channel attachment");
    assert!(
        error.to_string().contains("reopen the session"),
        "generation mismatch should require a fresh session: {error}"
    );
}

#[tokio::test]
async fn session_channel_half_close() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
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
        .expect("service session should open");

    let mut state = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned");
    let channel = state
        .session_channels
        .get_mut(&SessionChannelKey::new(&session.id, "cdp"))
        .expect("session open should create a channel state");

    channel.half_close_client_write("client_eof");

    assert_eq!(
        channel.client_to_target,
        SessionChannelHalfState::HalfClosed
    );
    assert_eq!(channel.target_to_client, SessionChannelHalfState::Open);
    assert!(
        channel
            .audit
            .iter()
            .any(|record| record.kind == SessionChannelAuditKind::ClientHalfClosed),
        "half-close should be audited"
    );
}

#[tokio::test]
async fn session_channel_backpressure() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
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
        .expect("service session should open");

    let mut state = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned");
    let channel = state
        .session_channels
        .get_mut(&SessionChannelKey::new(&session.id, "cdp"))
        .expect("session open should create a channel state");
    let high_watermark = channel.high_watermark_bytes;

    channel
        .enqueue_target_to_client_bytes(high_watermark)
        .expect("exact high-watermark write should be accepted");
    let error = channel
        .enqueue_target_to_client_bytes(1)
        .expect_err("bytes beyond the high watermark must apply backpressure");

    assert!(
        error.to_string().contains("backpressure"),
        "backpressure rejection should be explicit: {error}"
    );
    assert_eq!(channel.pending_target_to_client_bytes, high_watermark);
    assert!(
        channel
            .audit
            .iter()
            .any(|record| record.kind == SessionChannelAuditKind::Backpressure),
        "backpressure should be audited"
    );

    channel.drain_target_to_client_bytes(high_watermark / 2);
    assert_eq!(
        channel.pending_target_to_client_bytes,
        high_watermark - high_watermark / 2
    );
}

#[tokio::test]
async fn session_channel_disconnect_audit() {
    let tenant_id = TenantId::new("tenant").expect("tenant id should be valid");
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
        .expect("service session should open");

    let mut state = manager
        .state
        .lock()
        .expect("manager lock should not be poisoned");
    let channel = state
        .session_channels
        .get_mut(&SessionChannelKey::new(&session.id, "cdp"))
        .expect("session open should create a channel state");
    channel
        .enqueue_target_to_client_bytes(256)
        .expect("initial buffered bytes should be accepted");

    channel.disconnect("transport_closed");

    assert_eq!(channel.client_to_target, SessionChannelHalfState::Closed);
    assert_eq!(channel.target_to_client, SessionChannelHalfState::Closed);
    assert_eq!(channel.pending_target_to_client_bytes, 0);
    assert!(
        channel.audit.iter().any(|record| {
            record.kind == SessionChannelAuditKind::Disconnected
                && record.reason == "transport_closed"
        }),
        "disconnect should preserve the audit reason"
    );
}
