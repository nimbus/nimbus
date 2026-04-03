use super::*;

fn http_demo_functions() -> serde_json::Value {
    json!([
        {
            "name": "messages:byAuthor",
            "export": "byAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
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
                "limit": 20
            },
            "runtime_handler": null
        },
        {
            "name": "messages:maybeByAuthor",
            "export": "maybeByAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": null,
            "runtime_handler": "async (ctx, { author }) => {\n    const messages = author\n      ? await ctx.db\n        .query(\"messages\")\n        .withIndex(\"by_author\", (q) => q.eq(\"author\", author))\n        .take(20)\n      : await ctx.db.query(\"messages\").take(20);\n    return messages.slice(0, 20);\n  }",
            "runtime_bindings": {
                "internalScheduledFunctions": {
                    "type": "generated_reference_tree",
                    "visibility": "internal",
                    "reference_kind": "mutation"
                },
                "internal": {
                    "type": "generated_reference_tree",
                    "visibility": "internal"
                }
            }
        },
        {
            "name": "messages:byId",
            "export": "byId",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "get",
                "table": "messages",
                "id": { "$arg": "id" }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:uniqueByAuthor",
            "export": "uniqueByAuthor",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
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
            },
            "runtime_handler": null
        },
        {
            "name": "messages:exactByAuthorAndBody",
            "export": "exactByAuthorAndBody",
            "module": "messages",
            "kind": "query",
            "visibility": "public",
            "schedulable": false,
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
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendInternal",
            "export": "sendInternal",
            "module": "messages",
            "kind": "mutation",
            "visibility": "internal",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendViaAction",
            "export": "sendViaAction",
            "module": "messages",
            "kind": "action",
            "visibility": "public",
            "schedulable": false,
            "plan": {
                "type": "call_mutation",
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:scheduleSend",
            "export": "scheduleSend",
            "module": "messages",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_run_after",
                "delay_ms": { "$arg": "delayMs" },
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            },
            "runtime_handler": null
        },
        {
            "name": "messages:sendAndSchedule",
            "export": "sendAndSchedule",
            "module": "messages",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": null,
            "runtime_handler": "async (ctx, { author, body }) => {\n    const id = await ctx.db.insert(\"messages\", { author, body });\n    await ctx.scheduler.runAfter(\n      1_000,\n      internalScheduledFunctions.messages.sendInternal,\n      { author, body: `${body} (scheduled)` },\n    );\n    return id;\n  }",
            "runtime_bindings": {
                "internalScheduledFunctions": {
                    "type": "generated_reference_tree",
                    "visibility": "internal",
                    "reference_kind": "mutation"
                },
                "internal": {
                    "type": "generated_reference_tree",
                    "visibility": "internal"
                }
            }
        }
    ])
}

fn http_demo_routes() -> serde_json::Value {
    json!([
        {
            "method": "POST",
            "plan": {
                "type": "http_response",
                "response": {
                    "kind": "json",
                    "body": {
                        "id": {
                            "$result": {
                                "index": 0,
                                "path": ""
                            }
                        }
                    },
                    "status": 201
                },
                "operation": {
                    "type": "call_mutation",
                    "name": "messages:sendInternal",
                    "visibility": "internal",
                    "args": {
                        "author": {
                            "$request": {
                                "source": "json",
                                "path": "author"
                            }
                        },
                        "body": {
                            "$request": {
                                "source": "json",
                                "path": "body"
                            }
                        }
                    }
                }
            },
            "path": "/messages",
            "name": "http:inline:0"
        },
        {
            "method": "GET",
            "plan": {
                "type": "http_response",
                "response": {
                    "kind": "json",
                    "body": {
                        "$result": {
                            "index": 0,
                            "path": ""
                        }
                    }
                },
                "operation": {
                    "type": "call_query",
                    "name": "messages:byAuthor",
                    "visibility": "public",
                    "args": {
                        "author": {
                            "$request": {
                                "source": "query",
                                "name": "author"
                            }
                        }
                    }
                }
            },
            "path": "/messages/by-author",
            "name": "http:inline:1"
        }
    ])
}

fn http_demo_schema() -> serde_json::Value {
    json!({
        "tables": {
            "messages": {
                "fields": {
                    "author": { "kind": "string" },
                    "body": { "kind": "string" }
                },
                "indexes": [
                    { "name": "by_author", "fields": ["author"] }
                ]
            }
        }
    })
}

fn http_demo_runtime_bundle_source(
    functions: &serde_json::Value,
    routes: &serde_json::Value,
) -> String {
    let manifest_json = serde_json::to_string_pretty(&json!({
        "functions": functions,
        "routes": routes,
    }))
    .expect("http demo manifest should serialize");
    let source = r#"// Generated by @neovex/codegen. Do not edit by hand.
const manifest = __MANIFEST__;
const functionsByName = new Map(
  manifest.functions.map((definition) => [definition.name, definition]),
);
const routesByName = new Map(
  (manifest.routes ?? [])
    .filter((route) => typeof route.name === "string" && route.name.length > 0)
    .map((route) => [route.name, route]),
);
const runtimeHandlersByName = new Map(
  manifest.functions
    .filter((definition) => typeof definition.runtime_handler === "string")
    .map((definition) => [definition.name, compileRuntimeHandler(definition)]),
);

function createRuntimeContext(request) {
  return globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
}

function compileRuntimeHandler(definition) {
  const source = definition.runtime_handler;
  if (typeof source !== "string" || source.length === 0) {
    return null;
  }

  const runtimeBindings = materializeRuntimeBindings(definition.runtime_bindings);
  const bindingNames = Object.keys(runtimeBindings);
  const bindingValues = bindingNames.map((name) => runtimeBindings[name]);
  const invoke = new Function(
    ...bindingNames,
    "ctx",
    "args",
    "request",
    "return (" + source + ")(ctx, args, request);",
  );

  return (ctx, args, request) => invoke(...bindingValues, ctx, args, request);
}

function materializeRuntimeBindings(bindingDescriptors) {
  const bindings = {};
  for (const [name, descriptor] of Object.entries(bindingDescriptors ?? {})) {
    bindings[name] = materializeRuntimeBinding(descriptor);
  }
  return bindings;
}

function materializeRuntimeBinding(descriptor) {
  if (descriptor === null || typeof descriptor !== "object") {
    throw new Error("invalid runtime binding descriptor");
  }
  switch (descriptor.type) {
    case "generated_reference_tree":
      return createGeneratedReferenceTree({
        visibility: descriptor.visibility,
        kind: descriptor.reference_kind ?? undefined,
      });
    default:
      throw new Error(`unsupported runtime binding descriptor: ${descriptor.type}`);
  }
}

function createGeneratedReferenceTree(config, pathParts = []) {
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (property === "kind" && pathParts.length > 0 && config.kind !== undefined) {
          return config.kind;
        }
        if (property === "name" && pathParts.length > 0) {
          return referenceNameFromPath(pathParts);
        }
        if (property === "visibility" && pathParts.length > 0) {
          return config.visibility;
        }
        if (typeof property === "symbol") {
          return undefined;
        }
        return createGeneratedReferenceTree(config, [
          ...pathParts,
          String(property),
        ]);
      },
    },
  );
}

function referenceNameFromPath(pathParts) {
  return pathParts.length > 1
    ? `${pathParts.slice(0, -1).join(".")}:${pathParts.at(-1)}`
    : pathParts[0];
}

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isRuntimeQueryBuilder(value) {
  return isPlainObject(value)
    && typeof value.__builderId === "string"
    && value.__builderId.length > 0;
}

function isArgPlaceholder(value) {
  return isPlainObject(value)
    && typeof value.$arg === "string"
    && Object.keys(value).length === 1;
}

function resolveArgsTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveArgsTemplate(item, args));
  }
  if (isArgPlaceholder(template)) {
    if (!Object.prototype.hasOwnProperty.call(args, template.$arg)) {
      throw new Error(`convex function argument missing: ${template.$arg}`);
    }
    return args[template.$arg];
  }

  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveArgsTemplate(value, args);
  }
  return resolved;
}

async function executeQueryDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = runtimeHandlersByName.get(definition.name);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {}, request);
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedQueryPlan(ctx, plan);
}

function executePaginatedQueryDefinition(definition, request) {
  const runtimeHandler = runtimeHandlersByName.get(definition.name);
  if (runtimeHandler) {
    return Promise.resolve(runtimeHandler(createRuntimeContext(request), request.args ?? {}, request))
      .then((result) => {
        if (isRuntimeQueryBuilder(result)) {
          if (typeof request.page_size !== "number") {
            throw new Error("paginated runtime invocation missing page_size");
          }
          return globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_paginate", {
            builder_id: result.__builderId,
            page_size: request.page_size,
            cursor: request.cursor ?? null,
            session_id: request.kind + ":" + request.function_name,
          });
        }
        return result;
      });
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  if (typeof request.page_size !== "number") {
    throw new Error("paginated runtime invocation missing page_size");
  }
  return globalThis.__neovexAsyncHostValue("op_neovex_ctx_paginated_query", {
    query: plan,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
    session_id: request.kind + ":" + request.function_name,
  });
}

async function executeMutationDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = runtimeHandlersByName.get(definition.name);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {}, request);
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedMutationPlan(ctx, plan);
}

async function executeActionDefinition(definition, request) {
  const ctx = createRuntimeContext(request);
  const runtimeHandler = runtimeHandlersByName.get(definition.name);
  if (runtimeHandler) {
    return await runtimeHandler(ctx, request.args ?? {}, request);
  }
  const plan = resolveArgsTemplate(definition.plan, request.args ?? {});
  return await executeResolvedActionPlan(ctx, plan, request);
}

function isQueryShape(plan) {
  return isPlainObject(plan)
    && typeof plan.table === "string"
    && Array.isArray(plan.filters)
    && Object.prototype.hasOwnProperty.call(plan, "order")
    && Object.prototype.hasOwnProperty.call(plan, "limit");
}

function createConstraintBuilderFromPlan(builder, filters) {
  for (const filter of filters ?? []) {
    const field = builder.field(filter.field);
    switch (filter.op) {
      case "eq":
        builder.eq(field, filter.value);
        break;
      case "neq":
        builder.neq(field, filter.value);
        break;
      case "gt":
        builder.gt(field, filter.value);
        break;
      case "gte":
        builder.gte(field, filter.value);
        break;
      case "lt":
        builder.lt(field, filter.value);
        break;
      case "lte":
        builder.lte(field, filter.value);
        break;
      default:
        throw new Error(`unsupported convex filter op: ${filter.op}`);
    }
  }
  return builder;
}

function buildQueryFromPlan(ctx, query) {
  let builder = ctx.db.query(query.table);
  if (Array.isArray(query.filters) && query.filters.length > 0) {
    builder = builder.filter((q) => createConstraintBuilderFromPlan(q, query.filters));
  }
  if (query.order && typeof query.order.direction === "string") {
    builder = builder.order(query.order.direction);
  }
  return builder;
}

async function executeResolvedQueryPlan(ctx, plan) {
  if (isPlainObject(plan) && plan.type === "get") {
    return await ctx.db.get(plan.table, plan.id);
  }
  if (isPlainObject(plan) && plan.type === "first") {
    return await buildQueryFromPlan(ctx, plan.query).first();
  }
  if (isPlainObject(plan) && plan.type === "unique") {
    return await buildQueryFromPlan(ctx, plan.query).unique();
  }
  if (isQueryShape(plan)) {
    const builder = buildQueryFromPlan(ctx, plan);
    return typeof plan.limit === "number"
      ? await builder.take(plan.limit)
      : await builder.collect();
  }
  return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query", {
    query: plan,
    session_id: "convex-runtime-query-plan",
  });
}

async function executeResolvedMutationPlan(ctx, plan) {
  if (isPlainObject(plan) && plan.type === "insert") {
    return ctx.db.insert(plan.table, plan.fields);
  }
  if (isPlainObject(plan) && plan.type === "update") {
    return ctx.db.patch(plan.table, plan.id, plan.patch);
  }
  if (isPlainObject(plan) && plan.type === "delete") {
    return ctx.db.delete(plan.table, plan.id);
  }
  if (isPlainObject(plan) && plan.type === "schedule_run_after") {
    return ctx.scheduler.runAfter(
      plan.delay_ms,
      {
        kind: "mutation",
        name: plan.name,
        visibility: plan.visibility ?? "public",
      },
      plan.args ?? {},
    );
  }
  if (isPlainObject(plan) && plan.type === "schedule_run_at") {
    return ctx.scheduler.runAt(
      plan.timestamp_ms,
      {
        kind: "mutation",
        name: plan.name,
        visibility: plan.visibility ?? "public",
      },
      plan.args ?? {},
    );
  }
  if (isPlainObject(plan) && plan.type === "schedule_cancel") {
    return ctx.scheduler.cancel(plan.job_id);
  }
  if (isPlainObject(plan) && (plan.type === "get" || plan.type === "first" || plan.type === "unique")) {
    return await executeResolvedQueryPlan(ctx, plan);
  }
  if (isQueryShape(plan)) {
    return await executeResolvedQueryPlan(ctx, plan);
  }
  return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_mutation", {
    mutation: plan,
    session_id: "convex-runtime-mutation-plan",
  });
}

function invokeNamedDefinition(name, expectedKind, args, options = {}) {
  const definition = functionsByName.get(name);
  if (!definition) {
    throw new Error(`convex function not found: ${name}`);
  }
  if (definition.kind !== expectedKind) {
    throw new Error(
      `convex function kind mismatch for ${name}: expected ${expectedKind}, got ${definition.kind}`,
    );
  }

  const request = {
    kind: expectedKind,
    function_name: name,
    args,
    page_size: options.pageSize,
    cursor: options.cursor ?? null,
  };

  switch (expectedKind) {
    case "query":
      return executeQueryDefinition(definition, request);
    case "paginated_query":
      return executePaginatedQueryDefinition(definition, request);
    case "mutation":
      return executeMutationDefinition(definition, request);
    case "action":
      return executeActionDefinition(definition, request);
    default:
      throw new Error(`unsupported convex function kind: ${expectedKind}`);
  }
}

async function invokeNamedDefinitionLocally(request) {
  const definition = functionsByName.get(request.function_name);
  if (!definition) {
    throw new Error("convex function not found: " + request.function_name);
  }
  const requestVisibility =
    typeof request.visibility === "string" ? request.visibility : "public";
  if (definition.visibility !== requestVisibility) {
    throw new Error(
      "convex function "
        + request.function_name
        + " is "
        + definition.visibility
        + ", not "
        + requestVisibility,
    );
  }
  if (definition.kind !== request.kind) {
    throw new Error(
      "convex function kind mismatch for "
        + request.function_name
        + ": expected "
        + request.kind
        + ", got "
        + definition.kind,
    );
  }
  return invokeNamedDefinition(request.function_name, request.kind, request.args ?? {}, {
    pageSize: request.page_size,
    cursor: request.cursor ?? null,
  });
}

async function executeResolvedActionPlan(ctx, plan, request) {
  if (!isPlainObject(plan) || typeof plan.type !== "string") {
    return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
      action: plan,
      session_id: request.kind + ":" + request.function_name,
    });
  }

  switch (plan.type) {
    case "query":
      return await executeResolvedQueryPlan(ctx, plan.query);
    case "paginated_query":
      return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_paginated_query", {
        query: plan.query.query,
        page_size: plan.query.page_size,
        cursor: plan.query.after ?? null,
        session_id: request.kind + ":" + request.function_name,
      });
    case "mutation":
      return await executeResolvedMutationPlan(ctx, plan.mutation);
    case "call_query":
      return ctx.runQuery(
        { kind: "query", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "call_mutation":
      return ctx.runMutation(
        { kind: "mutation", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "call_action":
      return ctx.runAction(
        { kind: "action", name: plan.name, visibility: plan.visibility ?? "public" },
        plan.args ?? {},
      );
    case "schedule_run_after":
    case "schedule_run_at":
    case "schedule_cancel":
      return await executeResolvedMutationPlan(ctx, plan);
    default:
      return await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
        action: plan,
        session_id: request.kind + ":" + request.function_name,
      });
  }
}

globalThis.__neovexInvoke = async function (request) {
  try {
    const definition = functionsByName.get(request.function_name);
    if (definition) {
      return { status: "ok", value: await invokeNamedDefinitionLocally(request) };
    }

    const route = request.kind === "action"
      ? routesByName.get(request.function_name)
      : undefined;
    if (route) {
      return await globalThis.__neovexAsyncHostValue("op_neovex_http_route", {
        request,
        route,
      });
    }

    throw new Error(`convex function or route not found: ${request.function_name}`);
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return {
        status: "error",
        error: error.neovexHostError,
      };
    }
    throw error;
  }
};

globalThis.__neovexInvokeNamedLocal = invokeNamedDefinitionLocally;

export {};
"#;
    source.replace("__MANIFEST__", &manifest_json)
}

async fn query_messages_by_author(
    api: &HttpApiFixture<'_>,
    author: Option<&str>,
) -> serde_json::Value {
    let response = api
        .convex_named_query(
            "demo",
            "messages:maybeByAuthor",
            json!({ "author": author }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    response
        .json::<serde_json::Value>()
        .await
        .expect("messages query should parse")
}

async fn wait_for_message(api: &HttpApiFixture<'_>, author: &str, body: &str) -> serde_json::Value {
    timeout(Duration::from_secs(3), async {
        loop {
            let messages = query_messages_by_author(api, Some(author)).await;
            if messages.as_array().is_some_and(|items| {
                items.iter().any(|message| {
                    message["author"] == json!(author) && message["body"] == json!(body)
                })
            }) {
                return messages;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("expected demo flow message to arrive")
}

#[tokio::test]
async fn convex_http_demo_flow_matches_generated_app_behavior() {
    let functions = http_demo_functions();
    let routes = http_demo_routes();
    let bundle = http_demo_runtime_bundle_source(&functions, &routes);
    let registry = convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions,
        routes,
        Some(&bundle),
        None,
        Some(http_demo_schema()),
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

    let flow_author = "http-demo-flow";
    let action_body = "via-action";
    let scheduled_body = "via-schedule";
    let runtime_body = "via-runtime";
    let http_body = "via-http-action";
    let unique_author = "http-demo-unique";
    let unique_body = "only-one";

    let action = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": flow_author, "body": action_body }),
        )
        .await;
    assert_eq!(action.status(), StatusCode::OK);
    let action_id = action
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert!(action_id.as_str().is_some());

    let filtered = wait_for_message(&api, flow_author, action_body).await;
    assert!(filtered.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(flow_author) && message["body"] == json!(action_body)
        })
    }));

    let by_id = api
        .convex_named_query("demo", "messages:byId", json!({ "id": action_id }))
        .await;
    assert_eq!(by_id.status(), StatusCode::OK);
    let by_id_body = by_id
        .json::<serde_json::Value>()
        .await
        .expect("byId response should parse");
    assert_eq!(by_id_body["author"], json!(flow_author));
    assert_eq!(by_id_body["body"], json!(action_body));

    let scheduled = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleSend",
            json!({
                "author": flow_author,
                "body": scheduled_body,
                "delayMs": 0
            }),
        )
        .await;
    assert_eq!(scheduled.status(), StatusCode::OK);
    let scheduled_job = scheduled
        .json::<serde_json::Value>()
        .await
        .expect("scheduleSend response should parse");
    assert!(scheduled_job.as_str().is_some());
    let scheduled_messages = wait_for_message(&api, flow_author, scheduled_body).await;
    assert!(scheduled_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(scheduled_body))
    }));

    let runtime = api
        .convex_named_mutation(
            "demo",
            "messages:sendAndSchedule",
            json!({ "author": flow_author, "body": runtime_body }),
        )
        .await;
    assert_eq!(runtime.status(), StatusCode::OK);
    let runtime_id = runtime
        .json::<serde_json::Value>()
        .await
        .expect("sendAndSchedule response should parse");
    assert!(runtime_id.as_str().is_some());
    let runtime_messages = wait_for_message(&api, flow_author, runtime_body).await;
    assert!(runtime_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(runtime_body))
    }));
    let runtime_scheduled_messages =
        wait_for_message(&api, flow_author, "via-runtime (scheduled)").await;
    assert!(runtime_scheduled_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!("via-runtime (scheduled)"))
    }));

    let http_post = api
        .convex_http_json(
            "demo",
            reqwest::Method::POST,
            "/messages",
            json!({ "author": flow_author, "body": http_body }),
        )
        .await;
    assert_eq!(http_post.status(), StatusCode::CREATED);
    let http_post_body = http_post
        .json::<serde_json::Value>()
        .await
        .expect("httpAction post response should parse");
    assert!(http_post_body["id"].as_str().is_some());
    wait_for_message(&api, flow_author, http_body).await;

    let http_get = api
        .convex_http(
            "demo",
            reqwest::Method::GET,
            "/messages/by-author?author=http-demo-flow",
        )
        .await;
    assert_eq!(http_get.status(), StatusCode::OK);
    let http_get_body = http_get
        .json::<serde_json::Value>()
        .await
        .expect("httpAction get response should parse");
    assert!(http_get_body.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["body"] == json!(http_body))
    }));

    assert_eq!(
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": unique_author, "body": unique_body }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let unique = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": unique_author }),
        )
        .await;
    assert_eq!(unique.status(), StatusCode::OK);
    let unique_body_json = unique
        .json::<serde_json::Value>()
        .await
        .expect("unique query should parse");
    assert_eq!(unique_body_json["author"], json!(unique_author));
    assert_eq!(unique_body_json["body"], json!(unique_body));

    let exact = api
        .convex_named_query(
            "demo",
            "messages:exactByAuthorAndBody",
            json!({ "author": unique_author, "body": unique_body }),
        )
        .await;
    assert_eq!(exact.status(), StatusCode::OK);
    let exact_body_json = exact
        .json::<serde_json::Value>()
        .await
        .expect("exact query should parse");
    assert_eq!(exact_body_json["author"], json!(unique_author));
    assert_eq!(exact_body_json["body"], json!(unique_body));

    assert_eq!(
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": unique_author, "body": "second" }),
        )
        .await
        .status(),
        StatusCode::OK
    );

    let unique_conflict = api
        .convex_named_query(
            "demo",
            "messages:uniqueByAuthor",
            json!({ "author": unique_author }),
        )
        .await;
    assert_eq!(unique_conflict.status(), StatusCode::BAD_REQUEST);
    let unique_conflict_body = unique_conflict
        .json::<serde_json::Value>()
        .await
        .expect("duplicate unique query error should parse");
    assert!(
        unique_conflict_body["error"]
            .as_str()
            .is_some_and(|message| message.contains("multiple documents")),
        "{unique_conflict_body}"
    );

    let all_messages = query_messages_by_author(&api, None).await;
    assert!(all_messages.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|message| message["author"] == json!(flow_author))
    }));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_http_demo_action_then_http_post_and_follow_up_action_all_complete() {
    let functions = http_demo_functions();
    let routes = http_demo_routes();
    let bundle = http_demo_runtime_bundle_source(&functions, &routes);
    let registry = convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions,
        routes,
        Some(&bundle),
        None,
        Some(http_demo_schema()),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let author = "http-demo-probe";
    let action_body = "via-action";
    let http_body = "via-http-action";
    let second_action_body = "via-second-action";

    let action = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": action_body }),
        )
        .await;
    assert_eq!(action.status(), StatusCode::OK);
    wait_for_message(&api, author, action_body).await;

    let http_post = server
        .client()
        .request(
            reqwest::Method::POST,
            api.convex_http_url("demo", "/messages"),
        )
        .json(&json!({ "author": author, "body": http_body }))
        .send()
        .await
        .expect("httpAction post should resolve");
    assert_eq!(http_post.status(), StatusCode::CREATED);
    wait_for_message(&api, author, http_body).await;

    let second_action = timeout(
        Duration::from_secs(1),
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": second_action_body }),
        ),
    )
    .await
    .expect("second action should resolve");
    assert_eq!(second_action.status(), StatusCode::OK);
    wait_for_message(&api, author, second_action_body).await;
}
