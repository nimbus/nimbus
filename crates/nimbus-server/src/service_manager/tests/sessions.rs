use super::*;
#[tokio::test]
async fn session_routes_open_service_sessions_with_target_snapshot_and_audit() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, _token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let tenant_id = TenantId::new("tenanta").expect("tenant id should parse");
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("browser service definition should create");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager)
            .with_local_server_security(local_server_security)
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["cdp", "page"],
            "requestedTtlMs": 60000,
        }))
        .send()
        .await
        .expect("session open should send");
    assert_eq!(open.status(), StatusCode::CREATED);
    let open_body = open.json::<Value>().await.expect("open body should parse");
    let session_id = open_body["metadata"]["id"]
        .as_str()
        .expect("session id should be a string")
        .to_owned();
    assert!(session_id.starts_with("session-"));
    assert_ne!(
        session_id, "session-1",
        "session ids must be opaque rather than sequence-shaped"
    );
    assert_eq!(open_body["metadata"]["tenantId"], json!("tenanta"));
    assert_eq!(
        open_body["spec"]["target"]["service"]["name"],
        json!("browser")
    );
    assert_eq!(
        open_body["spec"]["targetSnapshot"]["service"]["backend"],
        json!("builtIn")
    );
    assert_eq!(
        open_body["spec"]["targetSnapshot"]["service"]["provider"],
        json!("browser")
    );
    assert_eq!(open_body["status"]["lifecycleState"], json!("open"));

    let get = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .bearer_auth("tenant-a-browser-session")
        .send()
        .await
        .expect("session get should send");
    assert_eq!(get.status(), StatusCode::OK);

    let unauthenticated_existing = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .send()
        .await
        .expect("unauthenticated existing-session get should send");
    assert_eq!(unauthenticated_existing.status(), StatusCode::UNAUTHORIZED);

    let unauthenticated_missing = server
        .client()
        .get(server.http_url("/api/sessions/session-missing"))
        .send()
        .await
        .expect("unauthenticated missing-session get should send");
    assert_eq!(unauthenticated_missing.status(), StatusCode::UNAUTHORIZED);

    let wrong_tenant_existing = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .bearer_auth("tenant-b-browser-session")
        .send()
        .await
        .expect("wrong-tenant existing-session get should send");
    assert_eq!(wrong_tenant_existing.status(), StatusCode::NOT_FOUND);

    let wrong_tenant_missing = server
        .client()
        .get(server.http_url("/api/sessions/session-missing"))
        .bearer_auth("tenant-b-browser-session")
        .send()
        .await
        .expect("wrong-tenant missing-session get should send");
    assert_eq!(wrong_tenant_missing.status(), StatusCode::NOT_FOUND);

    let no_session_permission_existing = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .bearer_auth("tenant-a-db")
        .send()
        .await
        .expect("same-tenant no-session-permission get should send");
    assert_eq!(
        no_session_permission_existing.status(),
        StatusCode::FORBIDDEN
    );

    let no_session_permission_missing = server
        .client()
        .get(server.http_url("/api/sessions/session-missing"))
        .bearer_auth("tenant-a-db")
        .send()
        .await
        .expect("same-tenant no-session-permission missing get should send");
    assert_eq!(
        no_session_permission_missing.status(),
        StatusCode::FORBIDDEN
    );

    let no_target_grant_existing = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .bearer_auth("tenant-a-browser-session-no-grant")
        .send()
        .await
        .expect("same-tenant no-target-grant get should send");
    assert_eq!(no_target_grant_existing.status(), StatusCode::FORBIDDEN);

    let list = server
        .client()
        .get(server.http_url("/api/sessions?tenantId=tenanta&state=open"))
        .bearer_auth("tenant-a-browser-session")
        .send()
        .await
        .expect("session list should send");
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list.json::<Value>().await.expect("list body should parse");
    assert_eq!(list_body["items"][0]["metadata"]["id"], json!(session_id));

    let ungranted_list = server
        .client()
        .get(server.http_url("/api/sessions?tenantId=tenanta&state=open"))
        .bearer_auth("tenant-a-browser-session-no-grant")
        .send()
        .await
        .expect("ungranted session list should send");
    assert_eq!(ungranted_list.status(), StatusCode::OK);
    let ungranted_list_body = ungranted_list
        .json::<Value>()
        .await
        .expect("ungranted list body should parse");
    assert_eq!(
        ungranted_list_body["items"]
            .as_array()
            .expect("items should be an array")
            .len(),
        0,
        "session list must filter sessions whose service target is not reachable by exact grant"
    );

    let close = server
        .client()
        .post(server.http_url(&format!("/api/sessions/{session_id}/close")))
        .bearer_auth("tenant-a-browser-session")
        .json(&json!({ "reason": "test_complete" }))
        .send()
        .await
        .expect("session close should send");
    assert_eq!(close.status(), StatusCode::OK);
    let close_body = close
        .json::<Value>()
        .await
        .expect("close body should parse");
    assert_eq!(close_body["status"]["lifecycleState"], json!("closed"));
    assert_eq!(close_body["status"]["closeReason"], json!("test_complete"));

    let service_scoped_open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session-service-scope")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["cdp"],
        }))
        .send()
        .await
        .expect("service-scoped session open should send");
    assert_eq!(service_scoped_open.status(), StatusCode::CREATED);
    let service_scoped_body = service_scoped_open
        .json::<Value>()
        .await
        .expect("service-scoped open body should parse");
    let service_scoped_session_id = service_scoped_body["metadata"]["id"]
        .as_str()
        .expect("service-scoped session id should be a string")
        .to_owned();

    let service_scoped_list = server
        .client()
        .get(server.http_url("/api/sessions?tenantId=tenanta&state=open"))
        .bearer_auth("tenant-a-browser-session-service-scope")
        .send()
        .await
        .expect("service-scoped session list should send");
    assert_eq!(service_scoped_list.status(), StatusCode::OK);
    let service_scoped_list_body = service_scoped_list
        .json::<Value>()
        .await
        .expect("service-scoped list body should parse");
    assert_eq!(
        service_scoped_list_body["items"][0]["metadata"]["id"],
        json!(service_scoped_session_id)
    );

    let service_scoped_get = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{service_scoped_session_id}")))
        .bearer_auth("tenant-a-browser-session-service-scope")
        .send()
        .await
        .expect("service-scoped session get should send");
    assert_eq!(service_scoped_get.status(), StatusCode::OK);

    let service_scoped_close = server
        .client()
        .post(server.http_url(&format!("/api/sessions/{service_scoped_session_id}/close")))
        .bearer_auth("tenant-a-browser-session-service-scope")
        .json(&json!({ "reason": "service_scoped_close" }))
        .send()
        .await
        .expect("service-scoped session close should send");
    assert_eq!(service_scoped_close.status(), StatusCode::OK);
    let service_scoped_close_body = service_scoped_close
        .json::<Value>()
        .await
        .expect("service-scoped close body should parse");
    assert_eq!(
        service_scoped_close_body["status"]["closeReason"],
        json!("service_scoped_close")
    );

    let unauthenticated_existing_close = server
        .client()
        .post(server.http_url(&format!("/api/sessions/{session_id}/close")))
        .json(&json!({ "reason": "second_close" }))
        .send()
        .await
        .expect("unauthenticated existing-session close should send");
    assert_eq!(
        unauthenticated_existing_close.status(),
        StatusCode::UNAUTHORIZED
    );

    let unauthenticated_missing_close = server
        .client()
        .post(server.http_url("/api/sessions/session-missing/close"))
        .json(&json!({ "reason": "missing" }))
        .send()
        .await
        .expect("unauthenticated missing-session close should send");
    assert_eq!(
        unauthenticated_missing_close.status(),
        StatusCode::UNAUTHORIZED
    );

    let wrong_tenant_existing_close = server
        .client()
        .post(server.http_url(&format!("/api/sessions/{session_id}/close")))
        .bearer_auth("tenant-b-browser-session")
        .json(&json!({ "reason": "wrong_tenant" }))
        .send()
        .await
        .expect("wrong-tenant existing-session close should send");
    assert_eq!(wrong_tenant_existing_close.status(), StatusCode::NOT_FOUND);

    let records = read_audit_records(&audit_log_path);
    assert!(records.iter().any(|record| {
        record.success
            && record.tenant_id.as_deref() == Some("tenanta")
            && record.auth_scope == "session_principal_class"
            && record.reason.contains("target=service:browser")
            && record.reason.contains("channels=cdp,page")
    }));
    assert!(records.iter().any(|record| {
        record.success
            && record.auth_scope == "session_principal_class"
            && record.reason.contains("reason=test_complete")
    }));
}

#[tokio::test]
async fn session_routes_reject_service_sessions_without_exact_grants_and_unsupported_channels() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let tenant_id = TenantId::new("tenanta").expect("tenant id should parse");
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("browser service definition should create");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager)
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let ungranted = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session-no-grant")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["cdp"],
        }))
        .send()
        .await
        .expect("ungranted session open should send");
    assert_eq!(ungranted.status(), StatusCode::FORBIDDEN);

    let wildcard_grant = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session-wildcard-grant")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["cdp"],
        }))
        .send()
        .await
        .expect("wildcard-grant session open should send");
    assert_eq!(wildcard_grant.status(), StatusCode::FORBIDDEN);

    let unsupported_channel = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session-stdio")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["stdio"],
        }))
        .send()
        .await
        .expect("unsupported channel session open should send");
    assert_eq!(unsupported_channel.status(), StatusCode::BAD_REQUEST);

    let ambiguous_target = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": {
                "service": { "name": "browser" },
                "sandbox": { "id": "sandbox-tenant-task" }
            },
            "channels": ["cdp"],
        }))
        .send()
        .await
        .expect("ambiguous session target open should send");
    assert_eq!(ambiguous_target.status(), StatusCode::BAD_REQUEST);

    let list = server
        .client()
        .get(server.http_url("/api/sessions?tenantId=tenanta"))
        .bearer_auth("tenant-a-browser-session")
        .send()
        .await
        .expect("session list should send");
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list.json::<Value>().await.expect("list body should parse");
    assert_eq!(
        list_body["items"]
            .as_array()
            .expect("items should be an array")
            .len(),
        0,
        "denied or unsupported opens must not create session records"
    );
}

#[tokio::test]
async fn session_routes_open_sandbox_sessions_by_id_and_expire_fail_closed() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let tenant_id = TenantId::new("tenanta").expect("tenant id should parse");
    let sandbox = manager
        .create_sandbox_resource_async(
            &tenant_id,
            "worker",
            standalone_sandbox_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("sandbox resource should create");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager)
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let by_name = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "sandbox": { "name": "task" } },
            "channels": ["stdio"],
        }))
        .send()
        .await
        .expect("sandbox-name session open should send");
    assert_eq!(by_name.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "sandbox": { "id": sandbox.id } },
            "channels": ["stdio", "files"],
            "requestedTtlMs": 100,
        }))
        .send()
        .await
        .expect("sandbox session open should send");
    assert_eq!(open.status(), StatusCode::CREATED);
    let open_body = open.json::<Value>().await.expect("open body should parse");
    let session_id = open_body["metadata"]["id"]
        .as_str()
        .expect("session id should be string")
        .to_owned();
    assert_eq!(
        open_body["spec"]["target"]["sandbox"]["id"],
        json!(sandbox.id)
    );
    assert_eq!(open_body["status"]["lifecycleState"], json!("open"));

    let sandbox_scoped_list = server
        .client()
        .get(server.http_url("/api/sessions?tenantId=tenanta&state=open"))
        .bearer_auth("tenant-a-sandbox-session-sandbox-scope")
        .send()
        .await
        .expect("sandbox-scoped session list should send");
    assert_eq!(sandbox_scoped_list.status(), StatusCode::OK);
    let sandbox_scoped_list_body = sandbox_scoped_list
        .json::<Value>()
        .await
        .expect("sandbox-scoped list body should parse");
    assert_eq!(
        sandbox_scoped_list_body["items"][0]["metadata"]["id"],
        json!(session_id)
    );

    let expired_body = wait_for_session_lifecycle_state(
        &server,
        &session_id,
        "tenant-a-sandbox-session",
        "expired",
    )
    .await;
    assert_eq!(expired_body["status"]["lifecycleState"], json!("expired"));
    assert_eq!(expired_body["status"]["closeReason"], json!("expired"));

    let close_expired = server
        .client()
        .post(server.http_url(&format!("/api/sessions/{session_id}/close")))
        .bearer_auth("tenant-a-sandbox-session")
        .json(&json!({ "reason": "late_client_close" }))
        .send()
        .await
        .expect("expired session close should send");
    assert_eq!(close_expired.status(), StatusCode::OK);
    let close_expired_body = close_expired
        .json::<Value>()
        .await
        .expect("expired close body should parse");
    assert_eq!(
        close_expired_body["status"]["lifecycleState"],
        json!("expired")
    );
    assert_eq!(
        close_expired_body["status"]["closeReason"],
        json!("expired")
    );
}

#[tokio::test]
async fn service_definition_delete_refuses_live_sessions_unless_force_closes_them() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let tenant_id = TenantId::new("tenanta").expect("tenant id should parse");
    manager
        .create_service_definition(
            &tenant_id,
            "browser",
            ServiceBackend::built_in("browser"),
            BTreeMap::new(),
        )
        .expect("browser service definition should create");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager)
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let open = server
        .client()
        .post(server.http_url("/api/sessions"))
        .bearer_auth("tenant-a-browser-session")
        .json(&json!({
            "tenantId": "tenanta",
            "target": { "service": { "name": "browser" } },
            "channels": ["cdp"],
            "requestedTtlMs": 60000,
        }))
        .send()
        .await
        .expect("browser session open should send");
    assert_eq!(open.status(), StatusCode::CREATED);
    let open_body = open.json::<Value>().await.expect("open body should parse");
    let session_id = open_body["metadata"]["id"]
        .as_str()
        .expect("session id should be a string")
        .to_owned();

    let normal_delete = server
        .client()
        .delete(server.http_url("/api/tenants/tenanta/services/browser?ifMatchGeneration=1"))
        .bearer_auth("tenant-a-browser-definition-force")
        .send()
        .await
        .expect("normal delete should send");
    assert_eq!(normal_delete.status(), StatusCode::CONFLICT);

    let force_delete = server
        .client()
        .delete(
            server.http_url("/api/tenants/tenanta/services/browser?ifMatchGeneration=1&force=true"),
        )
        .bearer_auth("tenant-a-browser-definition-force")
        .send()
        .await
        .expect("force delete should send");
    assert_eq!(force_delete.status(), StatusCode::NO_CONTENT);

    let session = server
        .client()
        .get(server.http_url(&format!("/api/sessions/{session_id}")))
        .bearer_auth("tenant-a-browser-session")
        .send()
        .await
        .expect("deleted-service session get should send");
    assert_eq!(session.status(), StatusCode::OK);
    let session_body = session
        .json::<Value>()
        .await
        .expect("session body should parse");
    assert_eq!(session_body["status"]["lifecycleState"], json!("closed"));
    assert_eq!(
        session_body["status"]["closeReason"],
        json!("service_force_deleted")
    );
}

async fn wait_for_session_lifecycle_state(
    server: &ServerFixture,
    session_id: &str,
    bearer_token: &str,
    expected_state: &str,
) -> Value {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        let response = server
            .client()
            .get(server.http_url(&format!("/api/sessions/{session_id}")))
            .bearer_auth(bearer_token)
            .send()
            .await
            .expect("session state probe should send");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<Value>()
            .await
            .expect("session state probe body should parse");
        if body["status"]["lifecycleState"] == json!(expected_state) {
            return body;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "session `{session_id}` did not reach state `{expected_state}` before timeout; last body: {body}"
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
