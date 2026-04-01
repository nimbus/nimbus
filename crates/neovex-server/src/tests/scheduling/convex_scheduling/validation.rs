use super::*;

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
