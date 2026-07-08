use super::*;

#[tokio::test]
async fn principal_class_service_definition_route_policy_allows_operator_cross_tenant_and_audits() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(service_manager(backend.clone()))
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let response = server
        .client()
        .get(server.http_url("/api/tenants/tenantb/services"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("operator cross-tenant service definition list request should send");

    assert_eq!(response.status(), StatusCode::OK);
    let records = read_audit_records(&audit_log_path);
    assert!(records.iter().any(|record| {
        record.success
            && record.tenant_id.as_deref() == Some("tenantb")
            && record.auth_scope == "service_definition_principal_class"
            && record.reason.contains("principal_class=operator")
            && record.reason.contains("service definition list authorized")
    }));
}

#[tokio::test]
async fn service_definition_routes_reject_body_conflicts_and_inline_credentials() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(service_manager(backend.clone()))
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let tenant_conflict = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&sandbox_service_definition_body("other", "worker"))
        .send()
        .await
        .expect("tenant conflict create should send");
    assert_eq!(tenant_conflict.status(), StatusCode::BAD_REQUEST);

    let credential_endpoint = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&external_service_definition_body(
            "tenant",
            "api",
            "https://user:secret@example.com",
        ))
        .send()
        .await
        .expect("external create should send");
    assert_eq!(credential_endpoint.status(), StatusCode::BAD_REQUEST);

    let hostless_endpoint = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&external_service_definition_body(
            "tenant",
            "hostless-api",
            "https://",
        ))
        .send()
        .await
        .expect("hostless external create should send");
    assert_eq!(hostless_endpoint.status(), StatusCode::BAD_REQUEST);

    let mut rootfs_definition = sandbox_service_definition_body("tenant", "rootfs-worker");
    rootfs_definition["spec"]["backend"]["sandbox"] = sandbox_rootfs_spec_body(
        "tenant",
        json!({ "kind": "service", "serviceName": "rootfs-worker" }),
    );
    let rootfs = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&rootfs_definition)
        .send()
        .await
        .expect("rootfs service definition create should send");
    assert_eq!(
        rootfs.status(),
        StatusCode::BAD_REQUEST,
        "public service definitions must reject host rootfs paths"
    );

    let mut build_definition = sandbox_service_definition_body("tenant", "build-worker");
    build_definition["spec"]["backend"]["sandbox"] = sandbox_build_spec_body(
        "tenant",
        json!({ "kind": "service", "serviceName": "build-worker" }),
    );
    let build = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&build_definition)
        .send()
        .await
        .expect("build service definition create should send");
    assert_eq!(
        build.status(),
        StatusCode::BAD_REQUEST,
        "public service definitions must reject local build context paths"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "rejected public host-path specs must not launch a backend"
    );
}
