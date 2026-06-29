use super::*;

#[tokio::test]
async fn convex_named_subscription_resolves_through_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:all",
            "kind": "query",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }
        },
        {
            "name": "tasks:create",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "tasks",
                "fields": {
                    "title": { "$arg": "title" }
                }
            }
        }
    ]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
    .await
    .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-named", "tasks:all", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-named"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Named convex insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Named convex insert"));
}

#[tokio::test]
async fn convex_named_subscription_uses_runtime_bundle_when_available() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "tasks:all",
                "kind": "query",
                "plan": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            },
            {
                "name": "tasks:create",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": {
                        "title": { "$arg": "title" }
                    }
                }
            }
        ]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  switch (request.function_name) {
    case "tasks:all":
      return {
        status: "ok",
        value: {
          runtime: true,
          value: await ctx.db.query("tasks").collect(),
        },
      };
    case "tasks:create":
      return {
        status: "ok",
        value: {
          runtime: true,
          value: await ctx.db.insert("tasks", { title: request.args.title }),
        },
      };
    default:
      throw new Error(`unexpected function: ${request.function_name}`);
  }
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
    .await
    .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-runtime", "tasks:all", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Runtime live insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(
        pushed["data"]["value"][0]["title"],
        json!("Runtime live insert")
    );
}
