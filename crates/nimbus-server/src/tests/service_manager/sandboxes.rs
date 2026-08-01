use super::*;

#[tokio::test]
async fn principal_class_sandbox_route_policy_allows_operator_cross_tenant_and_audits() {
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
        .post(server.http_url("/api/tenants/tenantb/sandboxes"))
        .bearer_auth(&token.token)
        .json(&sandbox_create_body("tenantb", "task"))
        .send()
        .await
        .expect("operator cross-tenant sandbox create request should send");

    assert_eq!(response.status(), StatusCode::CREATED);
    let records = read_audit_records(&audit_log_path);
    assert!(records.iter().any(|record| {
        record.success
            && record.tenant_id.as_deref() == Some("tenantb")
            && record.auth_scope == "sandbox_principal_class"
            && record.reason.contains("principal_class=operator")
            && record
                .reason
                .contains("sandbox create authorized with profile worker")
    }));
}

#[tokio::test]
async fn sandbox_resource_routes_are_id_addressed_and_do_not_publish_services() {
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

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenant/sandboxes"))
        .bearer_auth(&token.token)
        .json(&sandbox_create_body("tenant", "task"))
        .send()
        .await
        .expect("sandbox create should send");
    assert_eq!(create.status(), StatusCode::CREATED);
    let create_body = create.json::<Value>().await.expect("create should parse");
    let sandbox_id = create_body["metadata"]["id"]
        .as_str()
        .expect("sandbox id should be a string")
        .to_owned();
    assert_eq!(create_body["metadata"]["tenantId"], json!("tenant"));
    assert_eq!(create_body["spec"]["profile"], json!("worker"));
    assert_sandbox_resource_response_redacts_launch_details(&create_body);
    assert_eq!(
        create_body["status"]["conditions"][0]["type"],
        json!("Ready")
    );

    let service_lookup = server
        .client()
        .get(server.http_url("/api/tenants/tenant/services/task"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service lookup should send");
    assert_eq!(service_lookup.status(), StatusCode::NOT_FOUND);

    let list = server
        .client()
        .get(server.http_url("/api/tenants/tenant/sandboxes?labelKey=app&labelValue=task"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("sandbox list should send");
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list.json::<Value>().await.expect("list should parse");
    assert_eq!(list_body["items"][0]["metadata"]["id"], json!(sandbox_id));
    assert_sandbox_resource_response_redacts_launch_details(&list_body["items"][0]);

    let get = server
        .client()
        .get(server.http_url(&format!("/api/tenants/tenant/sandboxes/{sandbox_id}")))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("sandbox get should send");
    assert_eq!(get.status(), StatusCode::OK);
    let get_body = get.json::<Value>().await.expect("get should parse");
    assert_sandbox_resource_response_redacts_launch_details(&get_body);
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        1,
        "sandbox GET must not start or replace the inspected resource"
    );
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "sandbox GET must not command lifecycle teardown"
    );

    let stop = server
        .client()
        .post(server.http_url(&format!("/api/tenants/tenant/sandboxes/{sandbox_id}/stop")))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("sandbox stop should send");
    assert_eq!(stop.status(), StatusCode::OK);
    let stop_body = stop.json::<Value>().await.expect("stop should parse");
    assert_sandbox_resource_response_redacts_launch_details(&stop_body);
    assert_eq!(stop_body["status"]["lifecycleState"], json!("stopped"));
    assert_eq!(backend.stop_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sandbox_routes_enforce_owner_authority_and_backend_admission() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(service_manager(backend.clone()))
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let service_owned = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&service_owned_sandbox_create_body("tenanta", "db"))
        .send()
        .await
        .expect("service-owned sandbox create should send");
    assert_eq!(service_owned.status(), StatusCode::BAD_REQUEST);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 0);

    let mut wrong_backend_body = sandbox_create_body("tenanta", "task");
    wrong_backend_body["spec"]["backend"] = json!("container");
    let wrong_backend = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&wrong_backend_body)
        .send()
        .await
        .expect("wrong-backend sandbox create should send");
    assert_eq!(wrong_backend.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "backend-admission rejection must happen before backend start"
    );

    let label_only = server
        .client()
        .get(server.http_url("/api/tenants/tenanta/sandboxes?labelKey=app&labelValue=task"))
        .bearer_auth("tenant-a-db")
        .send()
        .await
        .expect("label-only sandbox list should send");
    assert_eq!(label_only.status(), StatusCode::FORBIDDEN);

    let task = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body("tenanta", "task"))
        .send()
        .await
        .expect("task sandbox create should send");
    assert_eq!(task.status(), StatusCode::CREATED);
    let other = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body("tenanta", "other"))
        .send()
        .await
        .expect("other sandbox create should send");
    assert_eq!(other.status(), StatusCode::CREATED);

    let exact_scoped_list = server
        .client()
        .get(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox-task-list")
        .send()
        .await
        .expect("exact-scoped sandbox list should send");
    assert_eq!(exact_scoped_list.status(), StatusCode::OK);
    let exact_scoped_body = exact_scoped_list
        .json::<Value>()
        .await
        .expect("exact-scoped list body should parse");
    assert_eq!(
        exact_scoped_body["items"]
            .as_array()
            .expect("items should be an array")
            .len(),
        1
    );
    assert_eq!(
        exact_scoped_body["items"][0]["metadata"]["id"],
        json!("sandbox-tenanta-task")
    );

    let prefix_scoped_list = server
        .client()
        .get(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox-task-prefix-list")
        .send()
        .await
        .expect("prefix-scoped sandbox list should send");
    assert_eq!(prefix_scoped_list.status(), StatusCode::OK);
    let prefix_scoped_body = prefix_scoped_list
        .json::<Value>()
        .await
        .expect("prefix-scoped list body should parse");
    assert_eq!(
        prefix_scoped_body["items"]
            .as_array()
            .expect("items should be an array")
            .len(),
        1
    );
    assert_eq!(
        prefix_scoped_body["items"][0]["metadata"]["id"],
        json!("sandbox-tenanta-task")
    );
}

#[tokio::test]
async fn sandbox_routes_reject_public_host_path_roots_before_launch() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(service_manager(backend.clone()))
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let rootfs = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body_with_spec(sandbox_rootfs_spec_body(
            "tenanta",
            json!({ "kind": "standalone", "displayName": "task" }),
        )))
        .send()
        .await
        .expect("rootfs sandbox create should send");
    assert_eq!(
        rootfs.status(),
        StatusCode::BAD_REQUEST,
        "public sandbox create must reject host rootfs paths"
    );

    let build = server
        .client()
        .post(server.http_url("/api/tenants/tenanta/sandboxes"))
        .bearer_auth("tenant-a-sandbox")
        .json(&sandbox_create_body_with_spec(sandbox_build_spec_body(
            "tenanta",
            json!({ "kind": "standalone", "displayName": "task" }),
        )))
        .send()
        .await
        .expect("build sandbox create should send");
    assert_eq!(
        build.status(),
        StatusCode::BAD_REQUEST,
        "public sandbox create must reject local build context paths"
    );
    assert_eq!(
        backend.image_starts.load(Ordering::SeqCst),
        0,
        "rejected public host-path sandbox specs must not launch a backend"
    );
}

#[tokio::test]
async fn sandbox_routes_mask_cross_tenant_sandbox_ids_as_not_found() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let manager = service_manager(backend.clone());
    let tenant_id = TenantId::new("tenanta").expect("tenant id should parse");
    let sandbox = manager
        .create_sandbox_resource_for_context_async(
            &crate::tenant::TenantIsolationContext::system(
                tenant_id.clone(),
                "sandbox.resource.create",
            ),
            "worker",
            standalone_sandbox_spec(&tenant_id, "task"),
            BTreeMap::new(),
        )
        .await
        .expect("tenant A sandbox should create");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(manager)
            .with_application_auth_verifier(Arc::new(StaticServiceRouteAuthVerifier))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let cross_tenant_get = server
        .client()
        .get(server.http_url(&format!("/api/tenants/tenantb/sandboxes/{}", sandbox.id)))
        .bearer_auth("tenant-b-sandbox")
        .send()
        .await
        .expect("cross-tenant sandbox get should send");
    assert_eq!(cross_tenant_get.status(), StatusCode::NOT_FOUND);

    let cross_tenant_stop = server
        .client()
        .post(server.http_url(&format!(
            "/api/tenants/tenantb/sandboxes/{}/stop",
            sandbox.id
        )))
        .bearer_auth("tenant-b-sandbox")
        .send()
        .await
        .expect("cross-tenant sandbox stop should send");
    assert_eq!(cross_tenant_stop.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "cross-tenant sandbox probes must not stop the probed sandbox"
    );
}
