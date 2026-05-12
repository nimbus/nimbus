use super::*;

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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext();
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
