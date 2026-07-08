use super::*;

#[tokio::test]
async fn service_definition_responses_redact_sandbox_launch_details() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend {
        image_starts: AtomicUsize::new(0),
        stop_calls: AtomicUsize::new(0),
    });
    let tenant_id = TenantId::new("tenant").expect("tenant id should parse");
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(engine.clone())
            .with_service_manager(service_manager_with_catalog(
                backend.clone(),
                BTreeMap::from([(
                    "builder".to_owned(),
                    sensitive_static_build_backend(&tenant_id, "builder"),
                )]),
            ))
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let create = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .json(&sandbox_service_definition_body("tenant", "worker"))
        .send()
        .await
        .expect("service definition create should send");
    assert_eq!(create.status(), StatusCode::CREATED);
    let create_body = create
        .json::<Value>()
        .await
        .expect("create response should parse");
    assert_response_redacts_launch_details(&create_body);
    assert_eq!(
        create_body["spec"]["backend"]["sandbox"]["root"]["kind"],
        json!("oci_image")
    );

    let list = server
        .client()
        .get(server.http_url("/api/tenants/tenant/services"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service definition list should send");
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list.json::<Value>().await.expect("list should parse");
    let builder = list_body["items"]
        .as_array()
        .expect("items should be an array")
        .iter()
        .find(|item| item["metadata"]["name"] == json!("builder"))
        .expect("static builder service definition should list");
    assert_response_redacts_launch_details(builder);
    assert_eq!(
        builder["spec"]["backend"]["sandbox"]["root"],
        json!({
            "kind": "redacted",
            "redacted": true,
            "reason": "operatorOnlyLaunchInput",
        })
    );
}

fn sensitive_static_build_backend(tenant_id: &TenantId, service_name: &str) -> ServiceBackend {
    let mut process = SandboxProcessSpec::new(vec![
        "runner".to_owned(),
        "--password=launch-secret".to_owned(),
    ]);
    process.entrypoint = Some(vec!["/bin/sh".to_owned()]);
    process.command = Some(vec!["-c".to_owned(), "echo $NIMBUS_SECRET".to_owned()]);
    process.env = vec!["NIMBUS_SECRET=launch-secret".to_owned()];
    ServiceBackend::sandbox(SandboxSpec::new(
        tenant_id.clone(),
        SandboxOwnerSpec::service(service_name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_build(
            "registry.example.com/private:latest",
            "/private/host/Dockerfile",
            "/private/host/context",
        ),
        process,
    ))
}

fn assert_response_redacts_launch_details(response: &Value) {
    let rendered = serde_json::to_string(response).expect("response should serialize");
    for forbidden in [
        "launch-secret",
        "NIMBUS_SECRET",
        "/private/host",
        "dockerfilePath",
        "contextPath",
        "registry.example.com/private",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "sandbox response leaked forbidden launch detail `{forbidden}`: {rendered}"
        );
    }

    let process = &response["spec"]["backend"]["sandbox"]["process"];
    assert!(
        process.get("env").is_none(),
        "sandbox response must not expose raw env values"
    );
    assert_eq!(process["argv"]["redacted"], json!(true));
    assert_eq!(process["environment"]["redacted"], json!(true));
}
