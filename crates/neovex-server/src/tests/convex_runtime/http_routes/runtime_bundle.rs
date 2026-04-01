use super::*;

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

globalThis.__neovexInvoke = async function(request) {
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_http_route", {
    request,
    route: routesByName.get(request.function_name),
  });
  return {
    status: "ok",
    value: {
      ...value,
      body: {
        runtime: true,
        value: value.body,
      },
    },
  };
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
