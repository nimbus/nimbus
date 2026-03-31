use std::fs;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::SystemTime;

use axum::{Json, Router, extract::State, routing::get};
use base64::Engine;
use neovex_core::{TableName, TenantId};
use neovex_engine::{Service, run_scheduler};
use neovex_runtime::{RuntimeBundle, RuntimeLimits};
use neovex_test_support::{HttpApiFixture, ServerFixture, ServiceFixture, WebSocketFixture};
use reqwest::StatusCode;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::json;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Duration, timeout};

use crate::{
    ConvexRegistry, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseSourceInfo,
    LicenseSourceKind, LicenseState, build_router, build_router_with_convex,
    build_router_with_license,
};

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_routes(functions, json!([]))
}

fn convex_registry_with_routes(
    functions: serde_json::Value,
    routes: serde_json::Value,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth(functions, routes, None, None)
}

fn convex_registry_with_routes_and_bundle(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth(functions, routes, bundle, None)
}

fn convex_registry_with_routes_and_bundle_and_auth(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
    auth_config: Option<serde_json::Value>,
) -> ConvexRegistry {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": functions }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": routes }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    if let Some(auth_config) = auth_config {
        fs::write(
            convex_dir.join("auth.config.json"),
            serde_json::to_vec_pretty(&auth_config).expect("convex auth json should serialize"),
        )
        .expect("convex auth config should write");
    }
    if let Some(bundle) = bundle {
        let bundle_path = convex_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("convex runtime bundle should write");
        let bundle_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("convex runtime bundle hash should write");
    }
    let registry =
        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
    std::mem::forget(tempdir);
    registry
}

async fn open_json_post_stream(
    server: &ServerFixture,
    path: &str,
    body: &serde_json::Value,
) -> TcpStream {
    let addr = server
        .http_url("")
        .trim_start_matches("http://")
        .to_string();
    let body = serde_json::to_string(body).expect("request body should serialize");
    let mut stream = TcpStream::connect(&addr)
        .await
        .expect("raw HTTP client should connect");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("raw HTTP request should write");
    stream.flush().await.expect("raw HTTP request should flush");
    stream
}

async fn wait_for_runtime_metrics(
    registry: &ConvexRegistry,
    description: &str,
    predicate: impl Fn(neovex_runtime::RuntimeMetricsSnapshot) -> bool,
) -> neovex_runtime::RuntimeMetricsSnapshot {
    let started_at = tokio::time::Instant::now();
    loop {
        let metrics = registry.runtime_metrics_snapshot();
        if predicate(metrics) {
            return metrics;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(3),
            "timed out waiting for {description}; last runtime metrics: {metrics:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[test]
fn convex_registry_requires_runtime_bundle_hash_sidecar() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    fs::write(
        convex_dir.join("bundle.mjs"),
        "globalThis.__neovexInvoke = async function () { return { status: \"ok\", value: null }; }; export {};",
    )
    .expect("convex runtime bundle should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("bundle without sidecar hash should be rejected");
    assert!(
        error.to_string().contains("bundle.sha256"),
        "unexpected registry error: {error}"
    );
}

#[tokio::test]
async fn health_route_returns_ok() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);
    let response = api.health().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("health json should parse")["ok"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn license_status_route_returns_community_defaults() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("community"));
    assert_eq!(body["status"], json!("community"));
    assert_eq!(body["source"]["kind"], json!("community_default"));
    assert_eq!(body["revenue_limit_usd"], json!(10_000_000));
    assert_eq!(body["monthly_active_user_limit"], json!(500));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
    assert_eq!(body["usage"]["limit"], json!(500));
    assert_eq!(body["usage"]["limit_exceeded"], json!(false));
}

#[tokio::test]
async fn license_status_route_returns_trial_license_details() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let license_state = LicenseState::from_document(
        LicenseDocument {
            schema_version: 1,
            kind: LicenseKind::Trial,
            issued_to: Some("Acme Corp".to_string()),
            issued_by: Some("Neovex".to_string()),
            issued_at_unix_ms: Some(1_700_000_000_000),
            expires_at_unix_ms: None,
            trial_expires_at_unix_ms: Some(u64::MAX),
            revenue_limit_usd: Some(10_000_000),
            monthly_active_user_limit: Some(500),
            entitlements: LicenseEntitlements {
                premium_support: true,
                custom_terms: true,
                ..LicenseEntitlements::default()
            },
            notes: None,
        },
        LicenseSourceInfo {
            kind: LicenseSourceKind::ExplicitFile,
            path: Some("/tmp/license.json".to_string()),
        },
    );
    let server =
        ServerFixture::start(build_router_with_license(fixture.service(), license_state)).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("trial"));
    assert_eq!(body["status"], json!("trial_active"));
    assert_eq!(body["issued_to"], json!("Acme Corp"));
    assert_eq!(body["source"]["kind"], json!("explicit_file"));
    assert_eq!(body["source"]["path"], json!("/tmp/license.json"));
    assert_eq!(body["entitlements"]["premium_support"], json!(true));
    assert_eq!(body["entitlements"]["custom_terms"], json!(true));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
}

#[tokio::test]
async fn license_status_route_tracks_global_monthly_active_users_across_tenants() {
    let issuer_one = "https://issuer-one.example.com";
    let issuer_two = "https://issuer-two.example.com";
    let application_id = "neovex-test";
    let (token_one, jwks_one) = issue_es256_test_token(
        issuer_one,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let (token_two, jwks_two) = issue_es256_test_token(
        issuer_two,
        application_id,
        "user-456",
        json!({ "email": "grace@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer_one,
                    "jwks": jwks_one,
                    "algorithm": "ES256",
                    "applicationID": application_id
                },
                {
                    "type": "customJwt",
                    "issuer": issuer_two,
                    "jwks": jwks_two,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("alpha").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.create_tenant("beta").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        server
            .client()
            .post(api.convex_url("alpha", "/query"))
            .json(&json!({ "name": "auth:whoami", "args": {} }))
            .send()
            .await
            .expect("unauthenticated alpha query should succeed")
            .status(),
        StatusCode::OK
    );

    for tenant_id in ["alpha", "beta"] {
        assert_eq!(
            server
                .client()
                .post(api.convex_url(tenant_id, "/query"))
                .header("Authorization", format!("Bearer {token_one}"))
                .json(&json!({ "name": "auth:whoami", "args": {} }))
                .send()
                .await
                .expect("authenticated token-one query should succeed")
                .status(),
            StatusCode::OK
        );
    }

    assert_eq!(
        server
            .client()
            .post(api.convex_url("beta", "/query"))
            .header("Authorization", format!("Bearer {token_two}"))
            .json(&json!({ "name": "auth:whoami", "args": {} }))
            .send()
            .await
            .expect("authenticated token-two query should succeed")
            .status(),
        StatusCode::OK
    );

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license usage json should parse");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(2));
    assert_eq!(usage["usage"]["limit_exceeded"], json!(false));
    assert!(
        usage.get("warnings").is_none() || usage["warnings"] == json!([]),
        "warnings should be empty when usage is comfortably below the limit"
    );
}

#[tokio::test]
async fn runtime_metrics_route_requires_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics json should parse");
    assert_eq!(body["limits"]["max_heap_mb"], json!(128));
    assert_eq!(body["limits"]["initial_heap_mb"], json!(8));
    assert_eq!(body["limits"]["execution_timeout_ms"], json!(30_000));
    assert!(body["limits"]["max_concurrent_isolates"].is_u64());
    assert_eq!(body["limits"]["max_nested_runtime_invocations"], json!(64));
    assert_eq!(body["metrics"]["worker_dispatched_invocations"], json!(0));
    assert_eq!(body["metrics"]["nested_local_dispatches"], json!(0));
    assert_eq!(
        body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(0)
    );
}

#[tokio::test]
async fn neovex_demo_html_is_served_without_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    let response = server
        .client()
        .get(server.http_url("/demos/neovex/html/"))
        .send()
        .await
        .expect("demo request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("demo body should load");
    assert!(body.contains("Neovex HTML Demo"));
    assert!(body.contains("Live tasks over HTTP writes and WebSocket subscriptions."));
}

#[tokio::test]
async fn convex_query_returns_documents_as_plain_json() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_query(
            "demo",
            json!({
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("convex query response should parse");
    assert_eq!(body[0]["title"], json!("Hello"));
    assert!(body[0]["_id"].is_string());
    assert!(body[0]["_creationTime"].is_u64());
}

#[tokio::test]
async fn convex_mutation_dispatches_existing_document_operations() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let insert = api
        .convex_mutation(
            "demo",
            json!({
                "type": "insert",
                "table": "tasks",
                "fields": { "title": "Inserted from convex" }
            }),
        )
        .await;
    assert_eq!(insert.status(), StatusCode::OK);
    let document_id = insert
        .json::<serde_json::Value>()
        .await
        .expect("convex mutation response should parse")
        .as_str()
        .expect("convex mutation insert should return a document id")
        .to_string();

    let list = api.list_documents("demo", "tasks").await;
    let body = list
        .json::<serde_json::Value>()
        .await
        .expect("document list should parse");
    assert_eq!(body["data"][0]["title"], json!("Inserted from convex"));

    let update = api
        .convex_mutation(
            "demo",
            json!({
                "type": "update",
                "table": "tasks",
                "id": document_id,
                "patch": { "title": "Updated from convex" }
            }),
        )
        .await;
    assert_eq!(update.status(), StatusCode::OK);

    let body = api
        .list_documents("demo", "tasks")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("updated list should parse");
    assert_eq!(body["data"][0]["title"], json!("Updated from convex"));
}

#[tokio::test]
async fn convex_action_can_execute_query_and_paginated_query_shapes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "Alpha" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let query = api
        .convex_action(
            "demo",
            json!({
                "type": "query",
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }),
        )
        .await;
    assert_eq!(query.status(), StatusCode::OK);
    assert_eq!(
        query
            .json::<serde_json::Value>()
            .await
            .expect("convex action query response should parse")[0]["title"],
        json!("Alpha")
    );

    let paginated = api
        .convex_action(
            "demo",
            json!({
                "type": "paginated_query",
                "query": {
                    "query": {
                        "table": "tasks",
                        "filters": [],
                        "order": null,
                        "limit": null
                    },
                    "page_size": 10,
                    "after": null
                }
            }),
        )
        .await;
    assert_eq!(paginated.status(), StatusCode::OK);
    let page = paginated
        .json::<serde_json::Value>()
        .await
        .expect("convex action paginated response should parse");
    assert_eq!(page["data"][0]["title"], json!("Alpha"));
    assert_eq!(page["has_more"], json!(false));
}

#[tokio::test]
async fn convex_named_query_and_mutation_resolve_from_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:byAuthor",
            "kind": "query",
            "plan": {
                "table": "messages",
                "filters": [
                    {
                        "field": "author",
                        "op": "eq",
                        "value": { "$arg": "author" }
                    }
                ],
                "order": null,
                "limit": null
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Grace", "body": "World" }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("named convex query response should parse");
    assert_eq!(
        body,
        json!([{
            "_creationTime": body[0]["_creationTime"].clone(),
            "_id": body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello"
        }])
    );
}

#[tokio::test]
async fn convex_named_query_uses_runtime_bundle_when_available() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:byAuthor", { name: "messages:byAuthor", kind: "query", plan: null }],
]);

globalThis.__neovexInvoke = function(request) {
  const response = globalThis.__neovexRawHostCall("convex.invoke", {
    request,
    definition: definitions.get(request.function_name),
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        runtime: true,
        value: response.value,
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Hello" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-backed named convex query response should parse");
    assert_eq!(body["runtime"], json!(true));
    assert_eq!(body["value"][0]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_named_query_can_use_ctx_query_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:byAuthor", {
    name: "messages:byAuthor",
    kind: "query",
    plan: {
      table: "messages",
      filters: [{ field: "author", op: "eq", value: { $arg: "author" } }],
      order: null,
      limit: null,
    },
  }],
]);

function resolveTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveTemplate(value, args);
  }
  return resolved;
}

globalThis.__neovexInvoke = function(request) {
  const definition = definitions.get(request.function_name);
  const response = globalThis.__neovexRawHostCall("convex.ctx.query", {
    query: resolveTemplate(definition.plan, request.args ?? {}),
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        ctx: true,
        value: response.value,
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Hello" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx query host-binding response should parse");
    assert_eq!(body["ctx"], json!(true));
    assert_eq!(body["value"][0]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_named_query_can_use_runtime_only_handler() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:maybeByAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:maybeByAuthor", {
    name: "messages:maybeByAuthor",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Hello" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Grace", "body": "World" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:maybeByAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only convex query response should parse");
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["author"], json!("Ada"));
    assert_eq!(body[0]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_runtime_only_query_reuses_same_isolate_for_ctx_run_query() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:outer",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { nested }) => { globalThis.__neovexCounter = (globalThis.__neovexCounter ?? 0) + 1; if (nested) { return await ctx.runQuery({ name: \"messages:outer\", visibility: \"public\" }, { nested: false }); } return globalThis.__neovexCounter; }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:outer", {
    name: "messages:outer",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { nested }) => { globalThis.__neovexCounter = (globalThis.__neovexCounter ?? 0) + 1; if (nested) { return await ctx.runQuery({ name: \"messages:outer\", visibility: \"public\" }, { nested: false }); } return globalThis.__neovexCounter; }",
  }],
]);

async function invokeLocal(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    throw new Error(`missing definition for ${request.function_name}`);
  }
  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );
  return await handler(
    globalThis.__neovexCreateContext({
      sessionId: `${request.kind}:${request.function_name}`,
    }),
    request.args ?? {},
    request,
  );
}

globalThis.__neovexInvoke = async function(request) {
  try {
    return { status: "ok", value: await invokeLocal(request) };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

globalThis.__neovexInvokeNamedLocal = invokeLocal;

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:outer", json!({ "nested": true }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("same-isolate nested runtime response should parse");
    assert_eq!(body, json!(2));
    let metrics_body = api
        .runtime_metrics()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics response should parse");
    assert_eq!(metrics_body["metrics"]["nested_local_dispatches"], json!(1));
    assert_eq!(
        metrics_body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(0)
    );
    assert_eq!(
        metrics_body["metrics"]["worker_dispatched_invocations"],
        json!(1)
    );
    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(metrics.nested_local_dispatches, 1);
    assert_eq!(metrics.fallback_cross_isolate_dispatches, 0);
    assert_eq!(metrics.worker_dispatched_invocations, 1);
}

#[tokio::test]
async fn convex_named_mutation_can_use_ctx_mutation_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:send",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "author": { "$arg": "author" },
                        "body": { "$arg": "body" }
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:send", {
    name: "messages:send",
    kind: "mutation",
    plan: {
      type: "insert",
      table: "messages",
      fields: {
        author: { $arg: "author" },
        body: { $arg: "body" },
      },
    },
  }],
]);

function resolveTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveTemplate(value, args);
  }
  return resolved;
}

globalThis.__neovexInvoke = function(request) {
  const definition = definitions.get(request.function_name);
  const response = globalThis.__neovexRawHostCall("convex.ctx.mutation", {
    mutation: resolveTemplate(definition.plan, request.args ?? {}),
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        ctx: true,
        value: response.value,
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx mutation host-binding response should parse");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("inserted documents should parse");
    assert_eq!(listed_body["data"][0]["author"], json!("Ada"));
    assert_eq!(listed_body["data"][0]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_named_mutation_can_use_runtime_only_handler() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendTwice",
                "kind": "mutation",
                "plan": null,
                "runtime_handler": "async (ctx, { body }) => { const firstId = await ctx.db.insert(\"messages\", { body }); await ctx.db.insert(\"messages\", { body: `${body} copy` }); return firstId; }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:sendTwice", {
    name: "messages:sendTwice",
    kind: "mutation",
    plan: null,
    runtime_handler: "async (ctx, { body }) => { const firstId = await ctx.db.insert(\"messages\", { body }); await ctx.db.insert(\"messages\", { body: `${body} copy` }); return firstId; }",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation("demo", "messages:sendTwice", json!({ "body": "Hello" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("runtime-only convex mutation response should parse")
            .as_str()
            .is_some(),
        "runtime-only mutation should return the first inserted id"
    );

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only mutation list should parse");
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn convex_named_query_can_use_bootstrapped_ctx_db_api() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db
    .query("messages")
    .filter((q) => q.eq(q.field("author"), request.args.author))
    .collect();
  return {
    status: "ok",
    value: {
      ctx: true,
      value,
    },
  };
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Hello from ctx.db" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("bootstrapped ctx.db response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert_eq!(body["value"][0]["body"], json!("Hello from ctx.db"));
}

#[tokio::test]
async fn convex_named_action_can_use_ctx_action_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "tasks:titles",
                "kind": "action",
                "plan": {
                    "type": "query",
                    "query": {
                        "table": "tasks",
                        "filters": [],
                        "order": { "field": "title", "direction": "asc" },
                        "limit": null
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["tasks:titles", {
    name: "tasks:titles",
    kind: "action",
    plan: {
      type: "query",
      query: {
        table: "tasks",
        filters: [],
        order: { field: "title", direction: "asc" },
        limit: null,
      },
    },
  }],
]);

globalThis.__neovexInvoke = function(request) {
  const definition = definitions.get(request.function_name);
  const response = globalThis.__neovexRawHostCall("convex.ctx.action", {
    action: definition.plan,
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        ctx: true,
        value: response.value,
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for title in ["Alpha", "Bravo"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .convex_named_action("demo", "tasks:titles", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx action host-binding response should parse");
    assert_eq!(body["ctx"], json!(true));
    assert_eq!(body["value"][0]["title"], json!("Alpha"));
    assert_eq!(body["value"][1]["title"], json!("Bravo"));
}

#[tokio::test]
async fn convex_named_mutation_can_use_bootstrapped_ctx_scheduler_api() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendInternal",
                "kind": "mutation",
                "visibility": "internal",
                "schedulable": true,
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:scheduleInternal",
                "kind": "mutation",
                "plan": {
                    "type": "schedule_run_after",
                    "delay_ms": { "$arg": "delayMs" },
                    "name": "messages:sendInternal",
                    "visibility": "internal",
                    "args": {
                        "body": { "$arg": "body" }
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__neovexInvoke = function(request) {
  const ctx = globalThis.__neovexCreateContext();
  return (async () => {
    const value = await ctx.scheduler.runAfter(
      request.args.delayMs,
      {
        kind: "mutation",
        name: "messages:sendInternal",
        visibility: "internal",
      },
      {
        body: request.args.body,
      },
    );
    return {
      status: "ok",
      value: {
        ctx: true,
        value,
      },
    };
  })();
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via ctx.scheduler",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("bootstrapped ctx.scheduler response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let documents = timeout(Duration::from_secs(2), async {
        loop {
            let response = api.list_documents("demo", "messages").await;
            let body = response
                .json::<serde_json::Value>()
                .await
                .expect("message list should parse");
            if body["data"].as_array().is_some_and(|documents| {
                documents
                    .iter()
                    .any(|document| document["body"] == json!("Scheduled via ctx.scheduler"))
            }) {
                break body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled ctx.scheduler mutation should complete");
    assert_eq!(
        documents["data"][0]["body"],
        json!("Scheduled via ctx.scheduler")
    );

    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
}

#[tokio::test]
async fn convex_named_query_reports_runtime_bundle_contract_errors() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some("export const noop = 1;\n"),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime contract error response should parse");
    assert!(
        body["error"]
            .as_str()
            .expect("error message should be a string")
            .contains("__neovexInvoke"),
        "unexpected runtime error body: {body}"
    );
}

#[tokio::test]
async fn convex_named_mutation_dispatches_compiled_patch_and_delete() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:rename",
            "kind": "mutation",
            "plan": {
                "type": "update",
                "table": "messages",
                "id": { "$arg": "id" },
                "patch": {
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:remove",
            "kind": "mutation",
            "plan": {
                "type": "delete",
                "table": "messages",
                "id": { "$arg": "id" }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let inserted_status = inserted.status();
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse");
    assert_eq!(inserted_status, StatusCode::OK, "{inserted_body}");
    let id = inserted_body
        .as_str()
        .expect("insert mutation should return a document id")
        .to_string();

    let renamed = api
        .convex_named_mutation(
            "demo",
            "messages:rename",
            json!({ "id": id, "body": "Edited" }),
        )
        .await;
    let renamed_status = renamed.status();
    let renamed_body = renamed
        .json::<serde_json::Value>()
        .await
        .expect("rename response should parse");
    assert_eq!(renamed_status, StatusCode::OK, "{renamed_body}");

    let after_rename = api.list_documents("demo", "messages").await;
    assert_eq!(after_rename.status(), StatusCode::OK);
    let after_rename_body = after_rename
        .json::<serde_json::Value>()
        .await
        .expect("documents should parse");
    assert_eq!(after_rename_body["data"][0]["body"], json!("Edited"));

    let deleted = api
        .convex_named_mutation(
            "demo",
            "messages:remove",
            json!({ "id": after_rename_body["data"][0]["_id"].clone() }),
        )
        .await;
    let deleted_status = deleted.status();
    let deleted_body = deleted
        .json::<serde_json::Value>()
        .await
        .expect("delete response should parse");
    assert_eq!(deleted_status, StatusCode::OK, "{deleted_body}");
    assert_eq!(deleted_body, serde_json::Value::Null);

    let after_delete = api
        .list_documents("demo", "messages")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("documents after delete should parse");
    assert_eq!(after_delete["data"], json!([]));
}

#[tokio::test]
async fn convex_named_query_can_return_single_document_or_null() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:byId",
            "kind": "query",
            "plan": {
                "type": "get",
                "table": "messages",
                "id": { "$arg": "id" }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let inserted_id = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")
        .as_str()
        .expect("insert should return id")
        .to_string();

    let found = api
        .convex_named_query("demo", "messages:byId", json!({ "id": inserted_id }))
        .await;
    assert_eq!(found.status(), StatusCode::OK);
    let found_body = found
        .json::<serde_json::Value>()
        .await
        .expect("get query response should parse");
    assert_eq!(found_body["author"], json!("Ada"));
    assert_eq!(found_body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:byId",
            json!({ "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing get query should parse"),
        serde_json::Value::Null
    );
}

#[tokio::test]
async fn convex_named_first_query_returns_single_document_or_null() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:latestByAuthor",
            "kind": "query",
            "plan": {
                "type": "first",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": {
                        "field": "author",
                        "direction": "desc"
                    },
                    "limit": 1
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let insert = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(insert.status(), StatusCode::OK);

    let response = api
        .convex_named_query(
            "demo",
            "messages:latestByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("named first query response should parse");
    assert_eq!(body["author"], json!("Ada"));
    assert_eq!(body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:latestByAuthor",
            json!({ "author": "Missing" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing named first query response should parse"),
        json!(null)
    );
}

#[tokio::test]
async fn convex_named_unique_query_returns_document_null_or_error() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:uniqueByAuthor",
            "kind": "query",
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let missing = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing unique query should parse"),
        json!(null)
    );

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );

    let single = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(single.status(), StatusCode::OK);
    let single_body = single
        .json::<serde_json::Value>()
        .await
        .expect("single unique query should parse");
    assert_eq!(single_body["author"], json!("Ada"));
    assert_eq!(single_body["body"], json!("Hello"));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Again" }),
        )
        .await
        .status()
        .is_success()
    );

    let duplicate = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;
    assert_eq!(duplicate.status(), StatusCode::BAD_REQUEST);
    let duplicate_body = duplicate
        .json::<serde_json::Value>()
        .await
        .expect("duplicate unique query error should parse");
    assert!(
        duplicate_body["error"]
            .as_str()
            .expect("error should be a string")
            .contains("multiple documents")
    );
}

#[tokio::test]
async fn convex_named_indexed_filter_unique_query_resolves_exact_match() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:exactByAuthorAndBody",
            "kind": "query",
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        },
                        {
                            "field": "body",
                            "op": "eq",
                            "value": { "$arg": "body" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let schema = json!({
        "table": "messages",
        "fields": [
            { "name": "author", "field_type": "string", "required": true },
            { "name": "body", "field_type": "string", "required": true }
        ],
        "indexes": [
            { "name": "by_author", "field": "author" }
        ]
    });
    assert_eq!(
        api.set_table_schema("demo", "messages", schema)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Other" }),
        )
        .await
        .status()
        .is_success()
    );

    let exact = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(exact.status(), StatusCode::OK);
    let body = exact
        .json::<serde_json::Value>()
        .await
        .expect("indexed unique query should parse");
    assert_eq!(body["author"], json!("Ada"));
    assert_eq!(body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": "Ada", "body": "Missing" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing indexed unique query should parse"),
        json!(null)
    );
}

#[tokio::test]
async fn convex_named_paginated_query_and_action_resolve_from_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:listPage",
            "kind": "paginated_query",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": { "field": "title", "direction": "asc" },
                "limit": null
            }
        },
        {
            "name": "tasks:titles",
            "kind": "action",
            "plan": {
                "type": "query",
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for title in ["Alpha", "Bravo", "Charlie"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let paginated = api
        .convex_named_paginated_query("demo", "tasks:listPage", json!({}), 2, None)
        .await;
    assert_eq!(paginated.status(), StatusCode::OK);
    let page = paginated
        .json::<serde_json::Value>()
        .await
        .expect("named convex paginated response should parse");
    assert_eq!(page["data"][0]["title"], json!("Alpha"));
    assert_eq!(page["data"][1]["title"], json!("Bravo"));
    assert_eq!(page["has_more"], json!(true));

    let action = api
        .convex_named_action("demo", "tasks:titles", json!({}))
        .await;
    assert_eq!(action.status(), StatusCode::OK);
    let body = action
        .json::<serde_json::Value>()
        .await
        .expect("named convex action response should parse");
    assert_eq!(body[0]["title"], json!("Alpha"));
    assert_eq!(body[2]["title"], json!("Charlie"));
}

#[tokio::test]
async fn convex_named_paginated_query_can_use_runtime_only_handler() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:listPage",
                "kind": "paginated_query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { const normalizedAuthor = author?.trim(); if (normalizedAuthor) { return ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), normalizedAuthor)); } return ctx.db.query(\"messages\"); }"
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const normalizedAuthor = request.args.author?.trim();
  const builder = normalizedAuthor
    ? ctx.db.query("messages").filter((q) => q.eq(q.field("author"), normalizedAuthor))
    : ctx.db.query("messages");
  const response = globalThis.__neovexRawHostCall("convex.ctx.db.query.paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for (author, body) in [("Ada", "Hello"), ("Ada", "Again"), ("Grace", "World")] {
        assert_eq!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "author": author, "body": body })
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .convex_named_paginated_query(
            "demo",
            "messages:listPage",
            json!({ "author": "Ada" }),
            1,
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only paginated convex response should parse");
    assert_eq!(page["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["data"][0]["author"], json!("Ada"));
    assert_eq!(page["has_more"], json!(true));
}

#[tokio::test]
async fn convex_runtime_only_query_can_run_runtime_only_query() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:inner",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)"
            },
            {
                "name": "messages:outer",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:inner", {
    name: "messages:inner",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)",
  }],
  ["messages:outer", {
    name: "messages:outer",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for (author, body) in [("Ada", "Hello"), ("Ada", "Again"), ("Bob", "Ignored")] {
        assert_eq!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "author": author, "body": body })
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .convex_named_query("demo", "messages:outer", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("nested runtime query response should parse");
    assert_eq!(body.as_array().map(Vec::len), Some(2));
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["author"] == json!("Ada"))
    );
    let metrics_body = api
        .runtime_metrics()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics response should parse");
    assert_eq!(
        metrics_body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["worker_dispatched_invocations"],
        json!(2)
    );
    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(metrics.fallback_cross_isolate_dispatches, 1);
    assert_eq!(metrics.worker_dispatched_invocations, 2);
}

#[tokio::test]
async fn convex_runtime_timeout_returns_request_timeout() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:spin",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const handler = new Function(
    "ctx",
    "args",
    "request",
    "return (async () => { while (true) {} })(ctx, args, request);",
  );
  return {
    status: "ok",
    value: await handler(globalThis.__neovexCreateContext(), request.args ?? {}, request),
  };
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        execution_timeout: Duration::from_millis(10),
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:spin", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime timeout response should parse");
    assert_eq!(body["error"], json!("operation canceled"));
}

#[tokio::test]
async fn dropped_runtime_http_request_cancels_runtime_invocation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:spin",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:spin", {
    name: "messages:spin",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => { while (true) {} }",
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }

  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );

  try {
    const value = await handler(
      globalThis.__neovexCreateContext(),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let request = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:spin", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "runtime invocation to start", |metrics| {
        metrics.active_isolates >= 1 && metrics.worker_dispatched_invocations >= 1
    })
    .await;

    drop(request);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "dropped runtime request cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 1,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.canceled_invocations, 1);
}

#[tokio::test]
async fn dropped_queued_runtime_request_never_starts_mutation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:block",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            },
            {
                "name": "messages:insertQueued",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:block", {
    name: "messages:block",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => { while (true) {} }",
  }],
  ["messages:insertQueued", {
    name: "messages:insertQueued",
    kind: "mutation",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })",
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }

  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );

  try {
    const value = await handler(
      globalThis.__neovexCreateContext(),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server =
        ServerFixture::start(build_router_with_convex(service.clone(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "blocking runtime query to start", |metrics| {
        metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1
    })
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        registry
            .runtime_metrics_snapshot()
            .worker_dispatched_invocations,
        1
    );

    drop(queued_mutation);
    drop(blocker);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing queued mutation table should succeed");
    assert!(documents.is_empty(), "queued mutation should never start");
}

#[tokio::test]
async fn convex_runtime_only_query_enforces_nested_runtime_budget() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:loop",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { depth }) => depth <= 0 ? [] : await ctx.runQuery({ name: \"messages:loop\", visibility: \"public\" }, { depth: depth - 1 })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:loop", {
    name: "messages:loop",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { depth }) => depth <= 0 ? [] : await ctx.runQuery({ name: \"messages:loop\", visibility: \"public\" }, { depth: depth - 1 })",
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }

  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );

  try {
    const value = await handler(
      globalThis.__neovexCreateContext(),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        max_nested_runtime_invocations: 2,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:loop", json!({ "depth": 3 }))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("nested runtime budget error should parse");
    assert!(
        body["error"]
            .as_str()
            .expect("error message should be a string")
            .contains("nested invocation limit exceeded"),
        "unexpected nested runtime budget error body: {body}"
    );
}

#[tokio::test]
async fn convex_runtime_only_action_can_run_runtime_only_mutation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:storeInternal",
                "kind": "mutation",
                "visibility": "internal",
                "plan": null,
                "runtime_handler": "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })"
            },
            {
                "name": "messages:sendViaRuntime",
                "kind": "action",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { author, body }) => await ctx.runMutation({ name: \"messages:storeInternal\", visibility: \"internal\" }, { author, body })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:storeInternal", {
    name: "messages:storeInternal",
    kind: "mutation",
    visibility: "internal",
    plan: null,
    runtime_handler: "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })",
  }],
  ["messages:sendViaRuntime", {
    name: "messages:sendViaRuntime",
    kind: "action",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { author, body }) => await ctx.runMutation({ name: \"messages:storeInternal\", visibility: \"internal\" }, { author, body })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action(
            "demo",
            "messages:sendViaRuntime",
            json!({ "author": "Ada", "body": "Nested runtime mutation" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("nested runtime action response should parse")
            .as_str()
            .is_some()
    );

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = listed
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    assert_eq!(listed["data"][0]["author"], json!("Ada"));
    assert_eq!(listed["data"][0]["body"], json!("Nested runtime mutation"));
}

#[tokio::test]
async fn convex_named_action_can_compose_query_mutation_and_action_calls() {
    let registry = convex_registry(json!([
        {
            "name": "messages:byAuthor",
            "kind": "query",
            "visibility": "public",
            "plan": {
                "table": "messages",
                "filters": [
                    {
                        "field": "author",
                        "op": "eq",
                        "value": { "$arg": "author" }
                    }
                ],
                "order": null,
                "limit": null
            }
        },
        {
            "name": "messages:storeInternal",
            "kind": "mutation",
            "visibility": "internal",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:listInternal",
            "kind": "action",
            "visibility": "internal",
            "plan": {
                "type": "call_query",
                "name": "messages:byAuthor",
                "visibility": "public",
                "args": {
                    "author": { "$arg": "author" }
                }
            }
        },
        {
            "name": "messages:sendViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_mutation",
                "name": "messages:storeInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:listViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_action",
                "name": "messages:listInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": "Ada", "body": "Hello from action" }),
        )
        .await;
    let inserted_status = inserted.status();
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert_eq!(inserted_status, StatusCode::OK, "{inserted_body}");
    assert!(inserted_body.as_str().is_some());

    let listed = api
        .convex_named_action("demo", "messages:listViaAction", json!({ "author": "Ada" }))
        .await;
    let listed_status = listed.status();
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("list via action response should parse");
    assert_eq!(listed_status, StatusCode::OK, "{listed_body}");
    assert_eq!(
        listed_body,
        json!([{
            "_creationTime": listed_body[0]["_creationTime"].clone(),
            "_id": listed_body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello from action"
        }])
    );
}

#[tokio::test]
async fn convex_http_routes_dispatch_compiled_http_actions() {
    let registry = convex_registry_with_routes(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "visibility": "public",
                "plan": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": null
                }
            },
            {
                "name": "messages:storeInternal",
                "kind": "mutation",
                "visibility": "internal",
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "author": { "$arg": "author" },
                        "body": { "$arg": "body" }
                    }
                }
            }
        ]),
        json!([
            {
                "method": "POST",
                "path": "/messages",
                "name": "http:inline:0",
                "plan": {
                    "operation": {
                        "type": "call_mutation",
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": {
                            "author": {
                                "$request": { "source": "json", "path": "author" }
                            },
                            "body": {
                                "$request": { "source": "json", "path": "body" }
                            }
                        }
                    },
                    "response": {
                        "kind": "json",
                        "body": {
                            "id": {
                                "$result": { "index": 0, "path": "" }
                            }
                        },
                        "status": 201
                    }
                }
            },
            {
                "method": "GET",
                "path_prefix": "/messages/by-author",
                "name": "http:inline:1",
                "plan": {
                    "operation": {
                        "type": "call_query",
                        "name": "messages:byAuthor",
                        "visibility": "public",
                        "args": {
                            "author": {
                                "$request": { "source": "query", "name": "author" }
                            }
                        }
                    },
                    "response": {
                        "kind": "json",
                        "body": {
                            "$result": { "index": 0, "path": "" }
                        }
                    }
                }
            }
        ]),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_http_json(
            "demo",
            reqwest::Method::POST,
            "/messages",
            json!({ "author": "Ada", "body": "Hello from httpAction" }),
        )
        .await;
    assert_eq!(inserted.status(), StatusCode::CREATED);
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("convex http post response should parse");
    assert!(inserted_body["id"].as_str().is_some());

    let listed = api
        .convex_http(
            "demo",
            reqwest::Method::GET,
            "/messages/by-author?author=Ada",
        )
        .await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("convex http get response should parse");
    assert_eq!(listed_body[0]["author"], json!("Ada"));
    assert_eq!(listed_body[0]["body"], json!("Hello from httpAction"));
}

#[tokio::test]
async fn convex_http_routes_use_runtime_bundle_when_available() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/healthz",
                "name": "http:inline:0",
                "plan": {
                    "response": {
                        "kind": "json",
                        "body": { "ok": true }
                    }
                }
            }
        ]),
        Some(
            r#"
const routesByName = new Map([
  ["http:inline:0", {
    name: "http:inline:0",
    method: "GET",
    path: "/healthz",
    plan: {
      response: {
        kind: "json",
        body: { ok: true },
      },
    },
  }],
]);

globalThis.__neovexInvoke = function(request) {
  const response = globalThis.__neovexRawHostCall("convex.http_route", {
    request,
    route: routesByName.get(request.function_name),
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        ...response.value,
        body: {
          runtime: true,
          value: response.value.body,
        },
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_http("demo", reqwest::Method::GET, "/healthz")
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-backed convex http response should parse");
    assert_eq!(body["runtime"], json!(true));
    assert_eq!(body["value"]["ok"], json!(true));
}

#[tokio::test]
async fn convex_http_routes_return_404_and_405_when_appropriate() {
    let registry = convex_registry_with_routes(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/healthz",
                "name": "http:inline:0",
                "plan": {
                    "response": {
                        "kind": "text",
                        "body": "ok"
                    }
                }
            }
        ]),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        api.convex_http("demo", reqwest::Method::GET, "/missing")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.convex_http("demo", reqwest::Method::POST, "/healthz")
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn convex_public_endpoints_reject_internal_functions() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:internalList",
            "kind": "query",
            "visibility": "internal",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "tasks:internalList", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("internal convex error should parse")["error"]
            .as_str()
            .expect("internal convex error should be a string")
            .contains("not public")
    );
}

#[tokio::test]
async fn create_tenant_and_run_document_lifecycle() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    assert_eq!(insert_response.status(), StatusCode::CREATED);
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    let update_response = api
        .update_document(
            "demo",
            "tasks",
            &document_id,
            serde_json::json!({ "title": "Updated" }),
        )
        .await;
    assert_eq!(update_response.status(), StatusCode::OK);
    assert_eq!(
        update_response
            .json::<serde_json::Value>()
            .await
            .expect("update response should parse")["id"],
        serde_json::json!(document_id)
    );

    let list_response = api.list_documents("demo", "tasks").await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = list_response
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    assert_eq!(list_body["data"][0]["title"], serde_json::json!("Updated"));

    let get_response = api.get_document("demo", "tasks", &document_id).await;
    assert_eq!(get_response.status(), StatusCode::OK);
    let get_body = get_response
        .json::<serde_json::Value>()
        .await
        .expect("get response should parse");
    assert_eq!(get_body["document"]["title"], serde_json::json!("Updated"));

    let delete_response = api.delete_document("demo", "tasks", &document_id).await;
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn rejects_invalid_document_id_and_tenant_name() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let invalid_tenant = api.create_tenant("../demo").await;
    assert_eq!(invalid_tenant.status(), StatusCode::BAD_REQUEST);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let invalid_document_id = api.get_document("demo", "tasks", "not-a-ulid").await;
    assert_eq!(invalid_document_id.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn commit_log_route_returns_sequenced_commits() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert_eq!(create_response.status(), StatusCode::CREATED);

    let insert_response = api
        .insert_document("demo", "tasks", serde_json::json!({ "title": "Hello" }))
        .await;
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    api.update_document(
        "demo",
        "tasks",
        &document_id,
        serde_json::json!({ "title": "Updated" }),
    )
    .await;

    let commit_log_response = api.commit_log("demo", None).await;
    assert_eq!(commit_log_response.status(), StatusCode::OK);
    let commit_log = commit_log_response
        .json::<serde_json::Value>()
        .await
        .expect("commit log response should parse");
    assert_eq!(commit_log["latest_sequence"], serde_json::json!(2));
    let commits = commit_log["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["sequence"], serde_json::json!(1));
    assert_eq!(
        commits[0]["writes"][0]["op_type"],
        serde_json::json!("insert")
    );
    assert_eq!(commits[1]["sequence"], serde_json::json!(2));
    assert_eq!(
        commits[1]["writes"][0]["op_type"],
        serde_json::json!("update")
    );

    let filtered_response = api.commit_log("demo", Some(1)).await;
    assert_eq!(filtered_response.status(), StatusCode::OK);
    let filtered = filtered_response
        .json::<serde_json::Value>()
        .await
        .expect("filtered response should parse");
    let commits = filtered["commits"]
        .as_array()
        .expect("commits should be an array");
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["sequence"], serde_json::json!(2));
}

#[tokio::test]
async fn duplicate_tenant_creation_returns_conflict() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let first = api.create_tenant("demo").await;
    let duplicate = api.create_tenant("demo").await;

    assert_eq!(first.status(), StatusCode::CREATED);
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn list_tenants_returns_all_known_tenants() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("bravo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.create_tenant("alpha").await.status(),
        StatusCode::CREATED
    );

    let response = api.list_tenants().await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("tenant list response should parse");
    assert_eq!(body["tenants"], json!(["alpha", "bravo"]));
}

#[tokio::test]
async fn delete_tenant_returns_no_content_and_removes_it_from_listing() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let list = api.list_tenants().await;
    let body = list
        .json::<serde_json::Value>()
        .await
        .expect("tenant list response should parse");
    assert_eq!(body["tenants"], json!([]));
}

#[tokio::test]
async fn operations_on_nonexistent_tenant_return_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);
    let document_id = neovex_core::DocumentId::new().to_string();

    assert_eq!(
        api.insert_document("missing", "tasks", json!({ "title": "Hello" }))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.list_documents("missing", "tasks").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.get_document("missing", "tasks", &document_id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.update_document(
            "missing",
            "tasks",
            &document_id,
            json!({ "title": "Updated" })
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete_document("missing", "tasks", &document_id)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.query_documents(
            "missing",
            json!({
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.commit_log("missing", None).await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.delete_tenant("missing").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn get_nonexistent_document_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .get_document("demo", "tasks", &neovex_core::DocumentId::new().to_string())
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn query_endpoint_returns_filtered_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Alpha", "status": "todo" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Beta", "status": "done" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .query_documents(
            "demo",
            json!({
                "table": "tasks",
                "filters": [{
                    "field": "status",
                    "op": "eq",
                    "value": "todo"
                }],
                "order": {
                    "field": "title",
                    "direction": "asc"
                },
                "limit": null
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    assert_eq!(
        body["data"],
        json!([{
            "_creationTime": body["data"][0]["_creationTime"].clone(),
            "_id": body["data"][0]["_id"].clone(),
            "status": "todo",
            "title": "Alpha"
        }])
    );
}

#[tokio::test]
async fn schema_crud_via_http() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true },
            { "name": "age", "field_type": "number", "required": false }
        ],
        "indexes": []
    });

    assert_eq!(
        api.set_table_schema("demo", "users", schema.clone())
            .await
            .status(),
        StatusCode::NO_CONTENT
    );

    let full_schema = api.get_schema("demo").await;
    assert_eq!(full_schema.status(), StatusCode::OK);
    let full_body = full_schema
        .json::<serde_json::Value>()
        .await
        .expect("schema response should parse");
    assert_eq!(full_body["tables"]["users"], schema);

    let table_schema = api.get_table_schema("demo", "users").await;
    assert_eq!(table_schema.status(), StatusCode::OK);
    let table_body = table_schema
        .json::<serde_json::Value>()
        .await
        .expect("table schema response should parse");
    assert_eq!(table_body, schema);

    let valid_insert = api
        .insert_document("demo", "users", json!({ "name": "Alice", "age": 30 }))
        .await;
    assert_eq!(valid_insert.status(), StatusCode::CREATED);

    let invalid_insert = api
        .insert_document("demo", "users", json!({ "age": "old" }))
        .await;
    assert_eq!(invalid_insert.status(), StatusCode::UNPROCESSABLE_ENTITY);

    assert_eq!(
        api.delete_table_schema("demo", "users").await.status(),
        StatusCode::NO_CONTENT
    );

    let permissive_insert = api
        .insert_document("demo", "users", json!({ "anything": { "goes": true } }))
        .await;
    assert_eq!(permissive_insert.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn paginated_query_endpoint_returns_pages() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let first_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": null
            }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page should parse");
    assert_eq!(
        first_body["data"]
            .as_array()
            .expect("data should be an array")
            .len(),
        2
    );
    assert_eq!(first_body["data"][0]["title"], json!("alpha"));
    assert_eq!(first_body["data"][1]["title"], json!("bravo"));
    assert_eq!(first_body["has_more"], json!(true));
    let cursor = first_body["next_cursor"]
        .as_str()
        .expect("next_cursor should be a string")
        .to_string();

    let second_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": cursor
            }),
        )
        .await;
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_body = second_page
        .json::<serde_json::Value>()
        .await
        .expect("second page should parse");
    assert_eq!(second_body["data"][0]["title"], json!("charlie"));
    assert_eq!(second_body["data"][1]["title"], json!("delta"));
    assert_eq!(second_body["has_more"], json!(true));
}

#[tokio::test]
async fn paginated_query_rejects_cursor_for_different_query_shape() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    for title in ["alpha", "bravo", "charlie"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let first_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                },
                "page_size": 2,
                "after": null
            }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page should parse");
    let cursor = first_body["next_cursor"]
        .as_str()
        .expect("next_cursor should be present")
        .to_string();

    let invalid_page = api
        .query_documents_paginated(
            "demo",
            json!({
                "query": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "desc" },
                    "limit": null
                },
                "page_size": 2,
                "after": cursor
            }),
        )
        .await;
    assert_eq!(invalid_page.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_endpoint_returns_range_filtered_results_with_indexed_schema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schema = json!({
        "table": "tasks",
        "fields": [
            { "name": "rank", "field_type": "number", "required": false }
        ],
        "indexes": [
            { "name": "by_rank", "field": "rank" }
        ]
    });
    assert_eq!(
        api.set_table_schema("demo", "tasks", schema).await.status(),
        StatusCode::NO_CONTENT
    );

    for rank in 0..10 {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "rank": rank }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .query_documents(
            "demo",
            json!({
                "table": "tasks",
                "filters": [
                    { "field": "rank", "op": "gte", "value": 3 },
                    { "field": "rank", "op": "lt", "value": 6 }
                ],
                "order": { "field": "rank", "direction": "asc" },
                "limit": null
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    let data = body["data"]
        .as_array()
        .expect("response data should be an array");
    assert_eq!(data.len(), 3);
    assert_eq!(data[0]["rank"], json!(3));
    assert_eq!(data[1]["rank"], json!(4));
    assert_eq!(data[2]["rank"], json!(5));
}

#[tokio::test]
async fn schedule_endpoint_returns_job_id_and_lists_pending_job() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 5_000,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Hello" }
                }
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse");
    assert!(body["job_id"].as_str().is_some());

    let jobs = api.list_scheduled_jobs("demo").await;
    assert_eq!(jobs.status(), StatusCode::OK);
    let jobs_body = jobs
        .json::<serde_json::Value>()
        .await
        .expect("scheduled jobs should parse");
    let jobs = jobs_body["jobs"]
        .as_array()
        .expect("jobs should be an array");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["mutation"]["type"], json!("insert"));
    assert_eq!(jobs[0]["mutation"]["table"], json!("tasks"));
}

#[tokio::test]
async fn schedule_endpoint_returns_not_found_for_unknown_tenant() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api
        .schedule_mutation(
            "missing",
            json!({
                "run_after_ms": 100,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Hello" }
                }
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_scheduled_job_endpoint_removes_pending_job() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 5_000,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Cancel me" }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);
    let job_id = schedule
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse")["job_id"]
        .as_str()
        .expect("job id should be present")
        .to_string();

    assert_eq!(
        api.cancel_scheduled_job("demo", &job_id).await.status(),
        StatusCode::NO_CONTENT
    );
    let jobs = api
        .list_scheduled_jobs("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("jobs should parse");
    assert_eq!(jobs["jobs"], json!([]));
}

#[tokio::test]
async fn convex_schedule_after_executes_named_public_mutation() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Convex scheduled" },
                "run_after_ms": 0
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);

    let history = timeout(Duration::from_secs(3), async {
        loop {
            let list = api.list_documents("demo", "messages").await;
            let body = list
                .json::<serde_json::Value>()
                .await
                .expect("list response should parse");
            let data = body["data"].as_array().expect("data should be an array");
            if !data.is_empty() {
                break data[0].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled mutation should execute");
    assert_eq!(history["body"], json!("Convex scheduled"));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_named_mutation_can_schedule_internal_generated_mutation() {
    let registry = convex_registry(json!([
        {
            "name": "messages:sendInternal",
            "kind": "mutation",
            "visibility": "internal",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:scheduleInternal",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_run_after",
                "delay_ms": { "$arg": "delayMs" },
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via handler",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("convex named mutation should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.as_str().is_some());

    let inserted = timeout(Duration::from_secs(3), async {
        loop {
            let body = api
                .list_documents("demo", "messages")
                .await
                .json::<serde_json::Value>()
                .await
                .expect("documents should parse");
            let data = body["data"].as_array().expect("data should be an array");
            if let Some(document) = data.first() {
                break document.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled internal mutation should execute");
    assert_eq!(inserted["body"], json!("Scheduled via handler"));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_ctx_mutation_host_binding_can_schedule_internal_generated_mutation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendInternal",
                "kind": "mutation",
                "visibility": "internal",
                "schedulable": true,
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:scheduleInternal",
                "kind": "mutation",
                "visibility": "public",
                "schedulable": true,
                "plan": {
                    "type": "schedule_run_after",
                    "delay_ms": { "$arg": "delayMs" },
                    "name": "messages:sendInternal",
                    "visibility": "internal",
                    "args": {
                        "body": { "$arg": "body" }
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:scheduleInternal", {
    name: "messages:scheduleInternal",
    kind: "mutation",
    plan: {
      type: "schedule_run_after",
      delay_ms: { $arg: "delayMs" },
      name: "messages:sendInternal",
      visibility: "internal",
      args: {
        body: { $arg: "body" },
      },
    },
  }],
]);

function resolveTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveTemplate(value, args);
  }
  return resolved;
}

globalThis.__neovexInvoke = function(request) {
  const definition = definitions.get(request.function_name);
  const response = globalThis.__neovexRawHostCall("convex.ctx.mutation", {
    mutation: resolveTemplate(definition.plan, request.args ?? {}),
  });
  if (response.status === "ok") {
    return {
      status: "ok",
      value: {
        ctx: true,
        value: response.value,
      },
    };
  }
  return response;
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via ctx.mutation host binding",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx mutation scheduler response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let inserted = timeout(Duration::from_secs(3), async {
        loop {
            let body = api
                .list_documents("demo", "messages")
                .await
                .json::<serde_json::Value>()
                .await
                .expect("scheduled documents should parse");
            if body["data"].as_array().is_some_and(|documents| {
                documents.iter().any(|document| {
                    document["body"] == json!("Scheduled via ctx.mutation host binding")
                })
            }) {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("runtime scheduler host-binding mutation should execute");
    assert!(
        inserted["data"]
            .as_array()
            .is_some_and(|documents| !documents.is_empty())
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_schedule_endpoints_reject_internal_mutations() {
    let registry = convex_registry(json!([
        {
            "name": "messages:internalSend",
            "kind": "mutation",
            "visibility": "internal",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:internalSend",
                "args": { "body": "Nope" },
                "run_after_ms": 0
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("schedule error should parse")["error"]
            .as_str()
            .expect("error should be a string")
            .contains("not public")
    );
}

#[tokio::test]
async fn convex_cancel_scheduled_job_removes_pending_named_mutation() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Later" },
                "run_after_ms": 60_000
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);
    let job_id = schedule
        .json::<serde_json::Value>()
        .await
        .expect("convex schedule response should parse")["job_id"]
        .as_str()
        .expect("convex job id should be present")
        .to_string();

    assert_eq!(
        api.convex_cancel_scheduled_job("demo", &job_id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let jobs = api
        .list_scheduled_jobs("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("jobs should parse");
    assert_eq!(jobs["jobs"], json!([]));
}

#[tokio::test]
async fn convex_named_mutation_can_cancel_scheduled_job() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "jobs:cancel",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_cancel",
                "job_id": { "$arg": "jobId" }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let scheduled = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Later" },
                "run_after_ms": 60_000
            }),
        )
        .await;
    assert_eq!(scheduled.status(), StatusCode::CREATED);
    let job_id = scheduled
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse")["job_id"]
        .as_str()
        .expect("job id should be present")
        .to_string();

    let cancelled = api
        .convex_named_mutation("demo", "jobs:cancel", json!({ "jobId": job_id }))
        .await;
    let status = cancelled.status();
    let body = cancelled
        .json::<serde_json::Value>()
        .await
        .expect("cancel response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, serde_json::Value::Null);

    let jobs = api
        .list_scheduled_jobs("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("jobs should parse");
    assert_eq!(jobs["jobs"], json!([]));
}

#[tokio::test]
async fn cron_endpoints_create_list_and_delete_jobs() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let create = api
        .create_cron_job(
            "demo",
            json!({
                "name": "heartbeat",
                "schedule": { "type": "interval", "seconds": 10 },
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "heartbeat" }
                }
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);

    let duplicate = api
        .create_cron_job(
            "demo",
            json!({
                "name": "heartbeat",
                "schedule": { "type": "interval", "seconds": 10 },
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "heartbeat" }
                }
            }),
        )
        .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let list = api.list_cron_jobs("demo").await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list
        .json::<serde_json::Value>()
        .await
        .expect("cron list should parse");
    let crons = list_body["crons"]
        .as_array()
        .expect("crons should be an array");
    assert_eq!(crons.len(), 1);
    assert_eq!(crons[0]["name"], json!("heartbeat"));
    assert_eq!(crons[0]["schedule"]["type"], json!("interval"));

    let delete = api.delete_cron_job("demo", "heartbeat").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let list = api.list_cron_jobs("demo").await;
    let list_body = list
        .json::<serde_json::Value>()
        .await
        .expect("cron list should parse");
    assert_eq!(list_body["crons"], json!([]));
}

#[tokio::test]
async fn scheduled_job_history_endpoint_reports_failures() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true }
        ],
        "indexes": []
    });
    assert_eq!(
        api.set_table_schema("demo", "users", schema).await.status(),
        StatusCode::NO_CONTENT
    );

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 0,
                "mutation": {
                    "type": "insert",
                    "table": "users",
                    "fields": { "age": 42 }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);
    let job_id = schedule
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse")["job_id"]
        .as_str()
        .expect("job_id should be present")
        .to_string();

    let history = timeout(Duration::from_secs(3), async {
        loop {
            let response = api.get_scheduled_job_result("demo", &job_id).await;
            if response.status() == StatusCode::OK {
                break response;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("history should become available");

    let body = history
        .json::<serde_json::Value>()
        .await
        .expect("history response should parse");
    assert_eq!(body["result"]["outcome"], json!("failed"));
    assert!(
        body["result"]["error"]
            .as_str()
            .expect("error should be present")
            .contains("schema validation error")
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_runtime_query_exposes_authenticated_identity_from_bearer_token() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({
            "email": "ada@example.com",
            "name": "Ada Lovelace",
            "role": "admin",
            "given_name": "Ada",
            "updated_at": 1710000000,
            "address": {
                "formatted": "123 Analytical Engine Way"
            }
        }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let unauthenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("unauthenticated auth query should succeed");
    assert_eq!(unauthenticated.status(), StatusCode::OK);
    assert_eq!(
        unauthenticated
            .json::<serde_json::Value>()
            .await
            .expect("unauthenticated auth body should parse"),
        json!(null)
    );

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated auth body should parse");
    assert_eq!(body["tokenIdentifier"], json!(format!("{issuer}|user-123")));
    assert_eq!(body["subject"], json!("user-123"));
    assert_eq!(body["issuer"], json!(issuer));
    assert_eq!(body["email"], json!("ada@example.com"));
    assert_eq!(body["name"], json!("Ada Lovelace"));
    assert_eq!(body["role"], json!("admin"));
    assert_eq!(body["given_name"], json!("Ada"));
    assert_eq!(body["updated_at"], json!(1710000000));
    assert_eq!(
        body["address.formatted"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body.get("givenName"), None);
    assert_eq!(body.get("updatedAt"), None);
    assert_eq!(body.get("address"), None);

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license status should parse after authenticated query");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(1));
}

#[tokio::test]
async fn convex_runtime_query_exposes_neovex_verified_identity_extension() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({
            "email": "ada@example.com",
            "name": "Ada Lovelace",
            "given_name": "Ada",
            "updated_at": 1710000000,
            "address": {
                "formatted": "123 Analytical Engine Way"
            },
            "role": "admin"
        }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ user: await ctx.auth.getUserIdentity(), verified: await ctx.auth.getVerifiedIdentity() })"
            }
        ]),
        json!([]),
        Some(runtime_verified_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated auth body should parse");
    assert_eq!(body["user"]["given_name"], json!("Ada"));
    assert_eq!(body["user"]["updated_at"], json!(1710000000));
    assert_eq!(
        body["user"]["address.formatted"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body["user"].get("givenName"), None);
    assert_eq!(body["user"].get("updatedAt"), None);
    assert_eq!(body["user"].get("address"), None);

    assert_eq!(body["verified"]["kind"], json!("custom_jwt"));
    assert_eq!(body["verified"]["name"], json!("Ada Lovelace"));
    assert_eq!(body["verified"]["givenName"], json!("Ada"));
    assert_eq!(body["verified"]["email"], json!("ada@example.com"));
    assert_eq!(body["verified"]["updatedAt"], json!("1710000000"));
    assert_eq!(
        body["verified"]["address"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body["verified"]["role"], json!("admin"));
    assert_eq!(body["verified"].get("given_name"), None);
    assert_eq!(body["verified"].get("updated_at"), None);
    assert_eq!(body["verified"].get("address.formatted"), None);
}

#[tokio::test]
async fn convex_runtime_query_rejects_invalid_bearer_token() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (_token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", "Bearer invalid.jwt.token")
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("invalid auth query should return an HTTP response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convex_websocket_auth_message_sets_runtime_identity() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:watchIdentity",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })"
            }
        ]),
        json!([]),
        Some(runtime_auth_subscription_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/convex/demo/ws"), "demo")
        .await
        .expect("browser-style websocket connection should succeed");
    socket
        .send_text(
            json!({
                "type": "authenticate",
                "token": token,
            })
            .to_string(),
        )
        .await;
    let authenticated = socket.next_json().await;
    assert_eq!(
        authenticated,
        json!({
            "type": "authenticated",
            "is_authenticated": true
        })
    );

    socket
        .subscribe_named("req-1", "auth:watchIdentity", json!({}))
        .await;
    let body = socket.next_json().await;
    assert_eq!(body["type"], json!("subscription_result"));
    assert_eq!(
        body["data"]["identity"]["tokenIdentifier"],
        json!(format!("{issuer}|user-123"))
    );
    assert_eq!(body["data"]["identity"]["email"], json!("ada@example.com"));
    assert_eq!(body["data"]["messages"][0]["body"], json!("Hello"));

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license status should parse after websocket auth");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(1));
}

#[tokio::test]
async fn convex_runtime_query_accepts_custom_jwt_issuer_without_scheme() {
    let provider_issuer = "https://issuer.example.com";
    let token_issuer = "issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token_with_audience(
        token_issuer,
        json!(application_id),
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": provider_issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
}

#[tokio::test]
async fn convex_runtime_query_accepts_eddsa_oidc_tokens_and_formats_address() {
    let application_id = "neovex-test";
    let (provider, token, _jwks) = mock_oidc_provider_with_token(
        json!(application_id),
        "user-123",
        json!({
            "name": "Ada Lovelace",
            "address": {
                "formatted": "123 Analytical Engine Way"
            }
        }),
    )
    .await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated OIDC query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated OIDC body should parse");
    assert_eq!(
        body["tokenIdentifier"],
        json!(format!("{}|user-123", provider.issuer()))
    );
    assert_eq!(body["address"], json!("123 Analytical Engine Way"));
}

#[tokio::test]
async fn convex_runtime_query_rejects_multi_audience_oidc_tokens() {
    let application_id = "neovex-test";
    let (provider, token, _jwks) = mock_oidc_provider_with_token(
        json!([application_id, "other-audience"]),
        "user-123",
        json!({}),
    )
    .await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("multi-audience OIDC query should return an HTTP response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convex_oidc_jwks_are_refetched_after_rotation() {
    let application_id = "neovex-test";
    let (provider, first_token, _first_jwks) =
        mock_oidc_provider_with_token(json!(application_id), "user-123", json!({})).await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let first_response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {first_token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("first OIDC query should succeed");
    assert_eq!(first_response.status(), StatusCode::OK);

    let (second_token, second_jwks) = issue_eddsa_test_token(
        provider.issuer(),
        json!(application_id),
        "user-456",
        json!({}),
    );
    provider.set_jwks(second_jwks);

    let second_response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {second_token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("second OIDC query should succeed after JWKS rotation");
    assert_eq!(second_response.status(), StatusCode::OK);
    assert!(provider.discovery_request_count() >= 2);
    assert!(provider.jwks_request_count() >= 2);
}

#[derive(Clone)]
struct MockOidcState {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
}

struct MockOidcProvider {
    issuer: String,
    jwks: Arc<Mutex<serde_json::Value>>,
    discovery_requests: Arc<AtomicUsize>,
    jwks_requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl MockOidcProvider {
    fn issuer(&self) -> &str {
        &self.issuer
    }

    fn set_jwks(&self, jwks: serde_json::Value) {
        *self
            .jwks
            .lock()
            .expect("mock oidc jwks lock should not be poisoned") = jwks;
    }

    fn discovery_request_count(&self) -> usize {
        self.discovery_requests.load(Ordering::SeqCst)
    }

    fn jwks_request_count(&self) -> usize {
        self.jwks_requests.load(Ordering::SeqCst)
    }
}

impl Drop for MockOidcProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn mock_oidc_discovery(State(state): State<MockOidcState>) -> Json<serde_json::Value> {
    state.discovery_requests.fetch_add(1, Ordering::SeqCst);
    Json(json!({
        "issuer": state.issuer,
        "jwks_uri": format!("{}/jwks", state.issuer),
    }))
}

async fn mock_oidc_jwks(State(state): State<MockOidcState>) -> Json<serde_json::Value> {
    state.jwks_requests.fetch_add(1, Ordering::SeqCst);
    Json(
        state
            .jwks
            .lock()
            .expect("mock oidc jwks lock should not be poisoned")
            .clone(),
    )
}

async fn start_mock_oidc_provider(initial_jwks: serde_json::Value) -> MockOidcProvider {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock OIDC listener should bind");
    let issuer = format!(
        "http://{}",
        listener
            .local_addr()
            .expect("mock OIDC listener should expose a local address")
    );
    let state = MockOidcState {
        issuer: issuer.clone(),
        jwks: Arc::new(Mutex::new(initial_jwks)),
        discovery_requests: Arc::new(AtomicUsize::new(0)),
        jwks_requests: Arc::new(AtomicUsize::new(0)),
    };
    let router = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(mock_oidc_discovery),
        )
        .route("/jwks", get(mock_oidc_jwks))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("mock OIDC server should run");
    });
    MockOidcProvider {
        issuer,
        jwks: state.jwks,
        discovery_requests: state.discovery_requests,
        jwks_requests: state.jwks_requests,
        task,
    }
}

async fn mock_oidc_provider_with_token(
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (MockOidcProvider, String, serde_json::Value) {
    let placeholder_jwks = json!({ "keys": [] });
    let provider = start_mock_oidc_provider(placeholder_jwks).await;
    let (token, jwks) = issue_eddsa_test_token(provider.issuer(), audience, subject, extra_claims);
    provider.set_jwks(jwks.clone());
    (provider, token, jwks)
}

fn runtime_auth_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => await ctx.auth.getUserIdentity()",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

fn runtime_verified_auth_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:whoami", {
    name: "auth:whoami",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ user: await ctx.auth.getUserIdentity(), verified: await ctx.auth.getVerifiedIdentity() })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

fn runtime_auth_subscription_bundle_source() -> &'static str {
    r#"
const definitions = new Map([
  ["auth:watchIdentity", {
    name: "auth:watchIdentity",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#
}

fn issue_es256_test_token(
    issuer: &str,
    application_id: &str,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, String) {
    issue_es256_test_token_with_audience(issuer, json!(application_id), subject, extra_claims)
}

fn issue_es256_test_token_with_audience(
    issuer: &str,
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, String) {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .expect("test key should generate");
    let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .expect("test key should parse");
    let header = json!({
        "alg": "ES256",
        "kid": "test-key",
        "typ": "JWT"
    });
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let mut claims = serde_json::Map::new();
    claims.insert("iss".to_string(), json!(issuer));
    claims.insert("sub".to_string(), json!(subject));
    claims.insert("aud".to_string(), audience);
    claims.insert("exp".to_string(), json!(now + 300));
    claims.insert("iat".to_string(), json!(now));
    if let serde_json::Value::Object(extra) = extra_claims {
        claims.extend(extra);
    }

    let header_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("jwt header should serialize"));
    let claims_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("jwt claims should serialize"));
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = key_pair
        .sign(&rng, signing_input.as_bytes())
        .expect("jwt signature should sign");
    let token = format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref())
    );

    let public_key = key_pair.public_key().as_ref();
    let jwks = json!({
        "keys": [
            {
                "kid": "test-key",
                "kty": "EC",
                "crv": "P-256",
                "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key[1..33]),
                "y": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&public_key[33..65]),
                "alg": "ES256",
                "use": "sig"
            }
        ]
    });
    let jwks_data_url = format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&jwks).expect("jwks should serialize"))
    );

    (token, jwks_data_url)
}

fn issue_eddsa_test_token(
    issuer: &str,
    audience: serde_json::Value,
    subject: &str,
    extra_claims: serde_json::Value,
) -> (String, serde_json::Value) {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("test key should generate");
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("test key should parse");
    let header = json!({
        "alg": "EdDSA",
        "kid": "test-key",
        "typ": "JWT"
    });
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let mut claims = serde_json::Map::new();
    claims.insert("iss".to_string(), json!(issuer));
    claims.insert("sub".to_string(), json!(subject));
    claims.insert("aud".to_string(), audience);
    claims.insert("exp".to_string(), json!(now + 300));
    claims.insert("iat".to_string(), json!(now));
    if let serde_json::Value::Object(extra) = extra_claims {
        claims.extend(extra);
    }

    let header_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&header).expect("jwt header should serialize"));
    let claims_segment = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).expect("jwt claims should serialize"));
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = key_pair.sign(signing_input.as_bytes());
    let token = format!(
        "{signing_input}.{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.as_ref())
    );

    let jwks = json!({
        "keys": [
            {
                "kid": "test-key",
                "kty": "OKP",
                "crv": "Ed25519",
                "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key_pair.public_key().as_ref()),
                "alg": "EdDSA",
                "use": "sig"
            }
        ]
    });

    (token, jwks)
}
