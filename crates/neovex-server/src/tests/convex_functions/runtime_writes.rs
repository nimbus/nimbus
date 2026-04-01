use super::super::*;

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

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_mutation", {
    mutation: resolveTemplate(definition.plan, request.args ?? {}),
    session_id: `${request.kind}:${request.function_name}`,
  });
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

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
    action: definition.plan,
    session_id: `${request.kind}:${request.function_name}`,
  });
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
