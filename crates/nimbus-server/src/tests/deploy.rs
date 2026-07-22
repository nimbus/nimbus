use super::*;

const DEPLOY_TOKEN: &str = "test-deploy-token";

fn deploy_router(engine: Arc<Engine>, registry: Option<ConvexRegistry>) -> axum::Router {
    // #41: the team gate guards every `/convex/<silo>` route these tests query
    // after deploying, and `convex_tenancy` is carried forward to *every*
    // activated generation — so provision the `demo` team tenancy
    // unconditionally (even when no initial registry is supplied). The static
    // verifier only matters for the initial deployment, so it stays gated on the
    // initial registry; post-activation the deployed bundle is its own verifier.
    let mut config = crate::router::RouterBuildConfig::core(engine)
        .with_deploy_admin_token(DEPLOY_TOKEN)
        .with_convex_tenancy(convex_team_tenancy_for("demo"));
    if let Some(registry) = registry {
        config = config
            .with_application_auth_verifier(std::sync::Arc::new(StaticConvexTeamVerifier))
            .with_convex(registry);
    }
    config.build()
}

fn deploy_router_with_system_registry(
    engine: Arc<Engine>,
    registry: Option<ConvexRegistry>,
) -> axum::Router {
    // #41: see `deploy_router` — provision the `demo` team tenancy
    // unconditionally so every activated generation is admissible.
    let mut config = crate::router::RouterBuildConfig::core(engine)
        .with_deploy_admin_token(DEPLOY_TOKEN)
        .with_convex_tenancy(convex_team_tenancy_for("demo"))
        .with_system_convex_registry(
            ConvexRegistry::from_embedded_system_bundle()
                .expect("embedded system Convex registry should load"),
        );
    if let Some(registry) = registry {
        config = config
            .with_application_auth_verifier(std::sync::Arc::new(StaticConvexTeamVerifier))
            .with_convex(registry);
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
            "convex": {
                "functions_json": { "functions": functions },
                "http_routes_json": { "routes": [] }
            }
        }
    })
}

fn cloud_functions_request(bundle: &str, bundle_sha256: &str) -> serde_json::Value {
    json!({
        "artifacts": {
            "cloud_functions": {
                "artifact_json": {
                    "version": 1,
                    "family": "cloud_functions",
                    "runtime_bundle": {
                        "entry_file": "bundle.mjs",
                        "sha256_file": "bundle.sha256"
                    },
                    "targets_manifest": "targets.json",
                    "import_resolution": {
                        "strategy": "deploy_alias_layer",
                        "covered_specifiers": [
                            "@google-cloud/functions-framework",
                            "firebase-admin/app",
                            "firebase-admin/firestore",
                            "firebase-functions/v2",
                            "firebase-functions/v2/firestore",
                            "firebase-functions/v2/https"
                        ]
                    }
                },
                "targets_json": {
                    "version": 1,
                    "targets": [{
                        "name": "syncUser",
                        "entrypoint": "exports.syncUser",
                        "authoring_surface": "firebase_v2",
                        "signature_type": "cloud_event",
                        "binding": {
                            "binding_kind": "firestore_document",
                            "event_type": "google.cloud.firestore.document.v1.written",
                            "database": "(default)",
                            "document": "users/{userId}",
                            "execution": "service_account"
                        }
                    }]
                },
                "bundle_mjs": bundle,
                "bundle_sha256": bundle_sha256
            }
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let registry = convex_registry(json!([]));
    let server = ServerFixture::start(
        crate::router::RouterBuildConfig::core(fixture.engine())
            .with_application_auth_verifier(crate::router::convex_application_auth_verifier(
                &registry,
            ))
            .with_convex(registry)
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
        body["error"]["message"]
            .as_str()
            .expect("error should be a string")
            .contains("deploy admin API is disabled")
    );
}

#[tokio::test]
async fn deploy_dry_run_validates_and_diffs_without_activation() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.engine(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["dry_run"] = json!(true);
            request["artifacts"]["convex"]["schema_json"] = schema_with_index("notes", "title");
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
    // #41: this test queries the silo *after* activation, where the verifier is
    // the deployed bundle (not the router's static verifier). So carry a real
    // custom-JWT bearer and deploy its matching auth config inside the bundle —
    // the activated generation verifies the token itself and the team tenancy
    // admits it on the verified issuer.
    let (team_bearer, team_auth_config) = convex_team_real_auth();
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(deploy_router_with_system_registry(
        fixture.engine(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, team_bearer);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["artifacts"]["convex"]["auth_config_json"] = team_auth_config;
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

    let bundles = api
        .convex_named_query(
            "_nimbus",
            "bundles:list",
            json!({ "status": null, "limit": null }),
        )
        .await;
    assert_eq!(bundles.status(), StatusCode::OK);
    let bundles = bundles
        .json::<serde_json::Value>()
        .await
        .expect("system bundles query should parse");
    let bundles = bundles.as_array().expect("bundles should be an array");
    assert_eq!(bundles.len(), 1);
    assert_eq!(bundles[0]["status"], json!("active"));
    assert_eq!(bundles[0]["sourceRef"], json!("deploy:generation:2"));

    let functions = api
        .convex_named_query(
            "_nimbus",
            "functions:list",
            json!({ "bundleId": null, "kind": null, "limit": null }),
        )
        .await;
    assert_eq!(functions.status(), StatusCode::OK);
    let functions = functions
        .json::<serde_json::Value>()
        .await
        .expect("system functions query should parse");
    let functions = functions.as_array().expect("functions should be an array");
    assert_eq!(functions.len(), 1);
    assert_eq!(functions[0]["path"], json!("notes:list"));
    assert_eq!(functions[0]["kind"], json!("query"));

    let runs = api
        .convex_named_query(
            "_nimbus",
            "runs:recent",
            json!({
                "bundleId": null,
                "functionPath": "notes:list",
                "status": null,
                "limit": null
            }),
        )
        .await;
    assert_eq!(runs.status(), StatusCode::OK);
    let runs = runs
        .json::<serde_json::Value>()
        .await
        .expect("system runs query should parse");
    let runs = runs.as_array().expect("runs should be an array");
    assert!(
        runs.iter().any(|run| {
            run["functionPath"] == json!("notes:list")
                && run["kind"] == json!("query")
                && run["status"] == json!("ok")
                && run["durationMs"].as_f64().is_some()
                && run["startedAt"].as_f64().is_some()
        }),
        "system runs should include the successful notes:list query: {runs:?}"
    );
}

#[tokio::test]
async fn deploy_validation_failure_leaves_previous_generation_live() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.engine(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        json!({
            "artifacts": {
                "convex": {
                    "functions_json": { "functions": [query_function("notes:list", "notes")] },
                    "bundle_mjs": "export const value = 1;\n",
                    "bundle_sha256": "definitely-not-the-sha256"
                }
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
async fn deploy_activation_accepts_cloud_functions_artifacts() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(deploy_router(fixture.engine(), None)).await;
    let bundle_source =
        "globalThis.__nimbusInvoke = async function () { return { ok: true }; };\nexport {};\n";
    let temp = tempfile::tempdir().expect("bundle tempdir should build");
    let bundle_path = temp.path().join("bundle.mjs");
    std::fs::write(&bundle_path, bundle_source).expect("bundle should write");
    let bundle_sha256 = nimbus_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
        .expect("bundle hash should compute");

    let response = deploy(
        &server,
        cloud_functions_request(bundle_source, &bundle_sha256),
        Some(DEPLOY_TOKEN),
    )
    .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("deploy response should be json");
    assert_eq!(status, StatusCode::OK, "unexpected deploy body: {body}");
    assert_eq!(body["activated"], json!(true));
    assert_eq!(body["generation"], json!(1));
    assert_eq!(body["previous_generation"], json!(0));
}

#[tokio::test]
async fn deploy_schema_validation_failure_leaves_previous_generation_live() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(deploy_router(
        fixture.engine(),
        Some(convex_registry(json!([query_function(
            "messages:list",
            "messages"
        )]))),
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = deploy(
        &server,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["artifacts"]["convex"]["schema_json"] = json!({
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

// CD7(j) — regression guard for `nimbus start` post-CD1.
//
// CD1 removed source-tree walk-up from `nimbus start`. The replacement
// contract is: a freshly-spawned daemon with no `--app-dir` must still
// accept deploys through the admin API, and the storage layer it shares
// with the previous daemon process must carry deploy artifacts forward
// across a restart. Two assertions document the contract:
//
//   1. Bundle records written by a deploy persist into the same data
//      directory, so a follow-up Service that opens that directory
//      sees the recorded generation in `_nimbus.bundles`. CD1 must not
//      have severed the link between the deploy admin API and the
//      tenant storage layer.
//
//   2. A freshly-spawned daemon on that data dir starts at
//      `generation = 0` and accepts a new deploy that re-creates a
//      live registry without needing `--app-dir`. Auto-activation of
//      the persisted bundle on startup is intentionally NOT asserted
//      here — that is a separate, not-yet-wired feature. This test
//      pins the current contract so a future autostart change can
//      relax assertion (2) honestly instead of silently.
#[tokio::test]
async fn deploy_persists_across_engine_restart_without_app_dir() {
    let data_dir = tempfile::tempdir().expect("data dir tempdir should create");
    // #41: the post-activation (and post-restart) queries hit the deployed
    // bundle's own verifier, so mint one real custom-JWT bearer + auth config and
    // deploy that config inside *both* bundles. Reusing the same config keeps the
    // two deploy artifacts byte-identical, which the sha256 dedup below relies on.
    let (team_bearer, team_auth_config) = convex_team_real_auth();

    {
        let engine_a = Arc::new(
            Engine::new(data_dir.path()).expect("first engine should build on shared data dir"),
        );
        let server_a =
            ServerFixture::start(deploy_router_with_system_registry(engine_a.clone(), None)).await;
        let api_a = HttpApiFixture::with_convex_bearer(&server_a, team_bearer.clone());
        assert_eq!(
            api_a.create_tenant("demo").await.status(),
            StatusCode::CREATED
        );

        let response = deploy(
            &server_a,
            {
                let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
                request["artifacts"]["convex"]["auth_config_json"] = team_auth_config.clone();
                request
            },
            Some(DEPLOY_TOKEN),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .json::<serde_json::Value>()
            .await
            .expect("first deploy response should be json");
        assert_eq!(body["activated"], json!(true));
        assert_eq!(body["previous_generation"], json!(0));
        assert_eq!(body["generation"], json!(1));

        assert_eq!(
            api_a
                .convex_named_query("demo", "notes:list", json!({}))
                .await
                .status(),
            StatusCode::OK,
            "deployed function must be reachable on the originating daemon"
        );

        let bundles_a = api_a
            .convex_named_query(
                "_nimbus",
                "bundles:list",
                json!({ "status": null, "limit": null }),
            )
            .await;
        assert_eq!(bundles_a.status(), StatusCode::OK);
        let bundles_a = bundles_a
            .json::<serde_json::Value>()
            .await
            .expect("first daemon bundles query should parse");
        let bundles_a = bundles_a
            .as_array()
            .expect("first daemon bundles should be an array");
        assert_eq!(bundles_a.len(), 1);
        assert_eq!(bundles_a[0]["status"], json!("active"));
        assert_eq!(bundles_a[0]["sourceRef"], json!("deploy:generation:1"));

        // Process exit releases the router's engine clone and every embedded
        // database lock atomically. Reproduce that boundary explicitly inside
        // this process before constructing the replacement daemon.
        drop(api_a);
        server_a.shutdown().await;
        engine_a.quiesce().await;
        drop(engine_a);
    }
    // `server_a` and `engine_a` drop here. The data dir survives — it is
    // owned by `data_dir` for the rest of the test, mimicking `nimbus start`
    // being killed while its persistent storage stays on disk.

    let engine_b = Arc::new(
        Engine::new(data_dir.path()).expect("second engine should reopen the same data dir"),
    );
    let server_b =
        ServerFixture::start(deploy_router_with_system_registry(engine_b.clone(), None)).await;
    let api_b = HttpApiFixture::with_convex_bearer(&server_b, team_bearer);

    // The deploy record written on engine A is durable in shared storage.
    // (Tenant durability is exercised implicitly: the redeploy below targets
    // the same `demo` tenant created on engine A and the subsequent named-
    // query against it must succeed.)
    let bundles_b_before = api_b
        .convex_named_query(
            "_nimbus",
            "bundles:list",
            json!({ "status": null, "limit": null }),
        )
        .await;
    if bundles_b_before.status() != StatusCode::OK {
        let status = bundles_b_before.status();
        let body = bundles_b_before
            .text()
            .await
            .expect("failed restart query response should have a body");
        panic!("restarted daemon bundles query returned {status}: {body}");
    }
    let bundles_b_before = bundles_b_before
        .json::<serde_json::Value>()
        .await
        .expect("restarted daemon bundles query should parse");
    let bundles_b_before = bundles_b_before
        .as_array()
        .expect("restarted daemon bundles should be an array");
    assert!(
        bundles_b_before
            .iter()
            .any(|bundle| bundle["sourceRef"] == json!("deploy:generation:1")),
        "engine A's deploy must persist in `_nimbus.bundles` post-restart: {bundles_b_before:?}"
    );

    // Auto-rehydration of the previously deployed bundle on startup is NOT
    // wired today; this assertion pins the current contract so a future
    // autostart change must update it deliberately. Without `--app-dir`, the
    // freshly-spawned daemon refuses Convex routes until a deploy lands.
    assert_eq!(
        api_b
            .convex_named_query("demo", "notes:list", json!({}))
            .await
            .status(),
        StatusCode::NOT_FOUND,
        "freshly-spawned daemon must not auto-activate persisted bundle (see CD7(j) docs)"
    );

    // Deploying on the restarted daemon must work with no source app dir —
    // this is the actual CD1 guarantee: the deploy admin path is independent
    // of any source-tree state on disk.
    let response = deploy(
        &server_b,
        {
            let mut request = deploy_request(json!([query_function("notes:list", "notes")]));
            request["artifacts"]["convex"]["auth_config_json"] = team_auth_config;
            request
        },
        Some(DEPLOY_TOKEN),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("restarted daemon deploy response should be json");
    assert_eq!(body["activated"], json!(true));
    assert_eq!(
        body["previous_generation"],
        json!(0),
        "restarted daemon should start at generation 0 (no auto-rehydrate)"
    );
    assert_eq!(body["generation"], json!(1));

    let redeployed_query = api_b
        .convex_named_query("demo", "notes:list", json!({}))
        .await;
    if redeployed_query.status() != StatusCode::OK {
        let status = redeployed_query.status();
        let body = redeployed_query
            .text()
            .await
            .expect("failed post-redeploy query response should have a body");
        panic!("deployed function query returned {status} post-redeploy: {body}");
    }

    let bundles_b_after = api_b
        .convex_named_query(
            "_nimbus",
            "bundles:list",
            json!({ "status": null, "limit": null }),
        )
        .await;
    if bundles_b_after.status() != StatusCode::OK {
        let status = bundles_b_after.status();
        let body = bundles_b_after
            .text()
            .await
            .expect("failed post-redeploy bundles response should have a body");
        panic!("post-redeploy bundles query returned {status}: {body}");
    }
    let bundles_b_after = bundles_b_after
        .json::<serde_json::Value>()
        .await
        .expect("post-redeploy bundles query should parse");
    let bundles_b_after = bundles_b_after
        .as_array()
        .expect("post-redeploy bundles should be an array");
    // The deploy artifacts are byte-identical across engine A and B, so
    // `_nimbus.bundles` deduplicates on sha256 and only one row remains.
    // The relevant durability assertion is that the row's status is
    // `active` post-redeploy and its sourceRef advances to the new
    // generation recorded by engine B.
    assert_eq!(
        bundles_b_after.len(),
        1,
        "post-redeploy `_nimbus.bundles` should hold the dedup'd row: {bundles_b_after:?}"
    );
    assert_eq!(bundles_b_after[0]["status"], json!("active"));
    assert_eq!(
        bundles_b_after[0]["sourceRef"],
        json!("deploy:generation:1"),
        "post-redeploy bundle sourceRef should reflect the restarted-daemon generation"
    );
}
