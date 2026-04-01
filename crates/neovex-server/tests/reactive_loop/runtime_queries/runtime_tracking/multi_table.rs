use super::*;

#[tokio::test]
async fn convex_runtime_multi_table_subscription_tracks_matching_writes_across_tables() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "dashboard:counts",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ openTasks: (await ctx.db.query(\"tasks\").filter((q) => q.eq(q.field(\"status\"), \"open\")).collect()).length, coreProfiles: (await ctx.db.query(\"profiles\").filter((q) => q.eq(q.field(\"team\"), \"core\")).collect()).length })"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(_request) {
  const ctx = globalThis.__neovexCreateContext();
  const openTasks = await ctx.db
    .query("tasks")
    .filter((q) => q.eq(q.field("status"), "open"))
    .collect();
  const coreProfiles = await ctx.db
    .query("profiles")
    .filter((q) => q.eq(q.field("team"), "core"))
    .collect();
  return {
    status: "ok",
    value: {
      runtime: true,
      openTasks: openTasks.length,
      coreProfiles: coreProfiles.length,
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

    assert!(api.create_tenant("demo").await.status().is_success());
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Tracked", "status": "open" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Closed", "status": "done" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document("demo", "profiles", json!({ "name": "Ada", "team": "core" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "profiles",
            json!({ "name": "Bob", "team": "support" })
        )
        .await
        .status()
        .is_success()
    );

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-runtime-multi", "dashboard:counts", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-multi"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["openTasks"], json!(1));
    assert_eq!(initial["data"]["coreProfiles"], json!(1));

    assert!(
        api.insert_document("demo", "messages", json!({ "body": "Ignored" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "multi-table runtime subscription should stay idle for unrelated tables"
    );

    assert!(
        api.insert_document(
            "demo",
            "profiles",
            json!({ "name": "Eve", "team": "support" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "multi-table runtime subscription should stay idle for non-matching writes on a tracked table"
    );

    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Second tracked", "status": "open" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["openTasks"], json!(2));
    assert_eq!(pushed["data"]["coreProfiles"], json!(1));

    assert!(
        api.insert_document("demo", "profiles", json!({ "name": "Lin", "team": "core" }))
            .await
            .status()
            .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["openTasks"], json!(2));
    assert_eq!(pushed["data"]["coreProfiles"], json!(2));
}
