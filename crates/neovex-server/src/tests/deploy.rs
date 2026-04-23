use super::*;

const DEPLOY_TOKEN: &str = "test-deploy-token";

fn deploy_router(service: Arc<Service>, registry: Option<ConvexRegistry>) -> axum::Router {
    let mut config =
        crate::router::RouterBuildConfig::core(service).with_deploy_admin_token(DEPLOY_TOKEN);
    if let Some(registry) = registry {
        config = config.with_convex(registry);
    }
    config.build()
}

fn query_function(name: &str, table: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "query",
        "plan": {
            "table": table,
            "filters": [],
            "order": null,
            "limit": 10
        }
    })
}

fn deploy_request(functions: serde_json::Value) -> serde_json::Value {
    json!({
        "artifacts": {
            "functions_json": { "functions": functions },
            "http_routes_json": { "routes": [] }
        }
    })
}

fn schema_with_index(table: &str, field: &str) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    fields.insert(field.to_string(), json!({ "kind": "string" }));
    let mut tables = serde_json::Map::new();
    tables.insert(
        table.to_string(),
        json!({
            "fields": fields,
            "indexes": [
                {
                    "name": format!("by_{field}"),
                    "fields": [field]
                }
            ]
        }),
    );
    json!({ "tables": tables })
}

async fn deploy(
    server: &ServerFixture,
    request: serde_json::Value,
    token: Option<&str>,
) -> reqwest::Response {
    let builder = server
        .client()
        .post(server.http_url("/api/admin/deploy"))
        .json(&request);
    let builder = if let Some(token) = token {
        builder.header("Authorization", format!("Bearer {token}"))
    } else {
        builder
    };
    builder.send().await.expect("deploy request should send")
}

#[tokio::test]
async fn deploy_admin_requires_configured_token() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(fixture.service())
            .with_convex(convex_registry(json!([])))
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let response = deploy(&server, deploy_request(json!([])), None).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("error response should be json");
    assert!(
        body["error"]
            .as_str()
            .expect("error should be a string")
            .contains("deploy admin API is disabled")
    );
}

#[tokio::test]
async fn deploy_dry_run_validates_and_diffs_without_activation() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.service(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["dry_run"] = json!(true);
            request["artifacts"]["schema_json"] = schema_with_index("notes", "title");
            request
        },
        Some(DEPLOY_TOKEN),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("deploy response should be json");
    assert_eq!(body["dry_run"], json!(true));
    assert_eq!(body["activated"], json!(false));
    assert_eq!(body["generation"], json!(1));
    assert_eq!(
        body["diff"]["functions"]["added"][0]["name"],
        json!("notes:list")
    );
    assert_eq!(
        body["diff"]["functions"]["removed"][0]["name"],
        json!("messages:list")
    );
    assert_eq!(body["diff"]["schema_changed"], json!(true));
    assert_eq!(body["diff"]["indexes_changed"], json!(true));

    assert_eq!(
        api.convex_named_query("demo", "messages:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_ne!(
        api.convex_named_query("demo", "notes:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn deploy_activation_swaps_new_requests_to_new_generation() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.service(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        deploy_request(json!([query_function("notes:list", "notes")])),
        Some(DEPLOY_TOKEN),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("deploy response should be json");
    assert_eq!(body["activated"], json!(true));
    assert_eq!(body["previous_generation"], json!(1));
    assert_eq!(body["generation"], json!(2));
    assert_eq!(
        body["diff"]["functions"]["added"][0]["name"],
        json!("notes:list")
    );

    assert_eq!(
        api.convex_named_query("demo", "notes:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_ne!(
        api.convex_named_query("demo", "messages:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn deploy_validation_failure_leaves_previous_generation_live() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.service(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        json!({
            "artifacts": {
                "functions_json": { "functions": [query_function("notes:list", "notes")] },
                "bundle_mjs": "export const value = 1;\n",
                "bundle_sha256": "definitely-not-the-sha256"
            }
        }),
        Some(DEPLOY_TOKEN),
    )
    .await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        api.convex_named_query("demo", "messages:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_ne!(
        api.convex_named_query("demo", "notes:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn deploy_schema_validation_failure_leaves_previous_generation_live() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.service(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["artifacts"]["schema_json"] = json!({
                "tables": {
                    "notes": {
                        "fields": {
                            "title": { "kind": "string" }
                        },
                        "indexes": [
                            {
                                "name": "by_missing",
                                "fields": ["missing"]
                            }
                        ]
                    }
                }
            });
            request
        },
        Some(DEPLOY_TOKEN),
    )
    .await;

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        api.convex_named_query("demo", "messages:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
    assert_ne!(
        api.convex_named_query("demo", "notes:list", json!({}))
            .await
            .status(),
        StatusCode::OK
    );
}
