use super::*;

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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return {
    status: "ok",
    value: {
      runtime: true,
      value: await ctx.db
        .query("messages")
        .filter((q) => q.eq(q.field("author"), request.args.author))
        .collect(),
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
