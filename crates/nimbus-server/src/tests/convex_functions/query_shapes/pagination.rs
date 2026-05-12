use super::super::super::*;

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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const normalizedAuthor = request.args.author?.trim();
  const builder = normalizedAuthor
    ? ctx.db.query("messages").filter((q) => q.eq(q.field("author"), normalizedAuthor))
    : ctx.db.query("messages");
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query_paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return {
    status: "ok",
    value,
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
async fn convex_runtime_only_full_scan_paginated_query_reuses_materialized_serving_snapshot() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:listPage",
                "kind": "paginated_query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.db.query(\"messages\")"
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const builder = ctx.db.query("messages");
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query_paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return {
    status: "ok",
    value,
  };
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = TableName::new("messages").expect("table name should build");

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for body in ["alpha", "beta"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                json!({ "body": body })
                    .as_object()
                    .expect("document should be an object")
                    .clone(),
            )
            .expect("fixture insert should succeed");
    }

    let first = api
        .convex_named_paginated_query("demo", "messages:listPage", json!({}), 1, None)
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_page = first
        .json::<serde_json::Value>()
        .await
        .expect("first runtime-only full scan page should parse");
    assert_eq!(first_page["data"].as_array().map(Vec::len), Some(1));

    let first_surface = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(first_surface.loaded_table_count, 1);
    assert_eq!(first_surface.table_load_count, 1);
    assert_eq!(first_surface.paginated_count, 1);

    let cursor = first_page["next_cursor"]
        .as_str()
        .expect("first page should include a continuation cursor");
    let second = api
        .convex_named_paginated_query("demo", "messages:listPage", json!({}), 1, Some(cursor))
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    let second_page = second
        .json::<serde_json::Value>()
        .await
        .expect("second runtime-only full scan page should parse");
    assert_eq!(second_page["data"].as_array().map(Vec::len), Some(1));

    let second_surface = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should reload");
    assert_eq!(second_surface.table_load_count, 1);
    assert_eq!(second_surface.paginated_count, 2);
    let snapshot_stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot stats should load");
    assert!(
        snapshot_stats.retained_snapshot_count >= 1,
        "runtime full-scan pagination should publish a retained serving snapshot"
    );
}
