use super::*;

struct ArmedBlockingFaultInjector {
    armed: std::sync::atomic::AtomicBool,
    inner: std::sync::Arc<neovex_test_support::BlockingFaultInjector>,
}

impl ArmedBlockingFaultInjector {
    fn new(point: neovex_storage::FaultPoint) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            armed: std::sync::atomic::AtomicBool::new(false),
            inner: neovex_test_support::BlockingFaultInjector::new(point),
        })
    }

    fn arm(&self) {
        self.armed.store(true, std::sync::atomic::Ordering::Release);
    }

    async fn wait_until_entered(&self) {
        self.inner.wait_until_entered().await;
    }

    fn release(&self) {
        self.armed
            .store(false, std::sync::atomic::Ordering::Release);
        self.inner.release();
    }
}

impl neovex_storage::FaultInjector for ArmedBlockingFaultInjector {
    fn check(&self, point: neovex_storage::FaultPoint) -> neovex_core::Result<()> {
        if !self.armed.load(std::sync::atomic::Ordering::Acquire) {
            return Ok(());
        }
        self.inner.check(point)
    }
}

fn http_demo_functions_with_runtime_delay(runtime_schedule_delay_ms: u64) -> serde_json::Value {
    let send_and_schedule_handler = format!(
        "async (ctx, {{ author, body }}) => {{\n    const id = await ctx.db.insert(\"messages\", {{ author, body }});\n    await ctx.scheduler.runAfter(\n      {runtime_schedule_delay_ms},\n      internalScheduledFunctions.messages.sendInternal,\n      {{ author, body: `${{body}} (scheduled)` }},\n    );\n    return id;\n  }}"
    );
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
            "runtime_handler": send_and_schedule_handler,
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

fn http_demo_registry(runtime_schedule_delay_ms: u64) -> ConvexRegistry {
    let functions = http_demo_functions_with_runtime_delay(runtime_schedule_delay_ms);
    let routes = http_demo_routes();
    let bundle = http_demo_runtime_bundle_source(&functions, &routes);
    convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions,
        routes,
        Some(&bundle),
        None,
        Some(http_demo_schema()),
    )
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MessageSnapshot {
    author: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreatedMessage {
    id: String,
    author: String,
    body: String,
}

#[derive(Debug, Clone)]
enum SeededDemoOperation {
    SendViaAction {
        author: String,
        body: String,
    },
    SendViaHttpAction {
        author: String,
        body: String,
    },
    ScheduleSend {
        author: String,
        body: String,
    },
    RuntimeSendAndSchedule {
        author: String,
        body: String,
    },
    QueryByAuthor {
        author: Option<String>,
    },
    LoadViaHttpAction {
        author: String,
    },
    LoadById {
        message_index: usize,
    },
    CheckUnique {
        author: String,
    },
    CheckExact {
        author: String,
        body: String,
        expect_match: bool,
    },
}

fn scenario_message_budget() -> usize {
    12
}

fn seeded_convex_demo_request_timeout() -> Duration {
    Duration::from_secs(3)
}

fn seeded_convex_demo_operation_count(step_count: usize) -> usize {
    (6 + step_count / 12).min(14)
}

fn seeded_convex_demo_faulted_overlap_step(operation_count: usize) -> usize {
    operation_count.saturating_sub(1).min(2)
}

fn seeded_convex_demo_draw(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut draw = *state;
    draw = (draw ^ (draw >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    draw = (draw ^ (draw >> 27)).wrapping_mul(0x94d049bb133111eb);
    draw ^ (draw >> 31)
}

fn seeded_convex_demo_author(state: &mut u64) -> String {
    const AUTHORS: [&str; 4] = ["Ada", "Byron", "Curie", "Dijkstra"];
    AUTHORS[(seeded_convex_demo_draw(state) as usize) % AUTHORS.len()].to_string()
}

fn seeded_convex_demo_body(seed: u64, step_index: usize, state: &mut u64) -> String {
    format!(
        "seed-{seed}-step-{step_index}-{:04x}",
        seeded_convex_demo_draw(state) & 0xffff
    )
}

fn normalize_message_snapshots(values: &[serde_json::Value]) -> Vec<MessageSnapshot> {
    let mut snapshots = values
        .iter()
        .map(|value| MessageSnapshot {
            author: value["author"]
                .as_str()
                .expect("message author should be a string")
                .to_string(),
            body: value["body"]
                .as_str()
                .expect("message body should be a string")
                .to_string(),
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}

fn expected_message_snapshots(
    created: &[CreatedMessage],
    author: Option<&str>,
) -> Vec<MessageSnapshot> {
    let mut snapshots = created
        .iter()
        .filter(|message| author.is_none_or(|expected| message.author == expected))
        .map(|message| MessageSnapshot {
            author: message.author.clone(),
            body: message.body.clone(),
        })
        .collect::<Vec<_>>();
    snapshots.sort();
    snapshots
}

fn message_from_value(value: &serde_json::Value) -> CreatedMessage {
    CreatedMessage {
        id: value["_id"]
            .as_str()
            .expect("message id should be a string")
            .to_string(),
        author: value["author"]
            .as_str()
            .expect("message author should be a string")
            .to_string(),
        body: value["body"]
            .as_str()
            .expect("message body should be a string")
            .to_string(),
    }
}

fn find_message_value(
    messages: &serde_json::Value,
    author: &str,
    body: &str,
) -> Option<serde_json::Value> {
    messages.as_array().and_then(|items| {
        items
            .iter()
            .find(|message| message["author"] == json!(author) && message["body"] == json!(body))
            .cloned()
    })
}

async fn wait_for_message_record(
    api: &HttpApiFixture<'_>,
    author: &str,
    body: &str,
) -> CreatedMessage {
    let messages = wait_for_message(api, author, body).await;
    let message = find_message_value(&messages, author, body)
        .expect("waited-for message should be present in the query response");
    message_from_value(&message)
}

fn seeded_convex_demo_context(
    seed: u64,
    operation_count: usize,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match case {
        Some(case) => {
            let step_suffix = step_index
                .map(|step| format!(" at convex demo step {step}"))
                .unwrap_or_default();
            format!(
                "{invariant}{step_suffix}; convex demo seed {}, operations {}. {}",
                seed,
                operation_count,
                case.failure_context("neovex-server", test_name, invariant)
            )
        }
        None => history_context(seed, operation_count, invariant, step_index),
    }
}

fn history_context(
    seed: u64,
    operation_count: usize,
    invariant: &str,
    step_index: Option<usize>,
) -> String {
    match step_index {
        Some(step_index) => format!(
            "{invariant}; convex demo seed {seed}, operations {operation_count}, step {step_index}"
        ),
        None => format!("{invariant}; convex demo seed {seed}, operations {operation_count}"),
    }
}

fn choose_seeded_convex_demo_operation(
    seed: u64,
    step_index: usize,
    created: &[CreatedMessage],
    state: &mut u64,
) -> SeededDemoOperation {
    if created.is_empty() {
        return SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        };
    }

    let max_messages = scenario_message_budget();
    let can_write = created.len() < max_messages;
    let can_runtime_write = created.len() + 2 <= max_messages;
    let draw = seeded_convex_demo_draw(state) % 10;

    match draw {
        0 if can_runtime_write => SeededDemoOperation::RuntimeSendAndSchedule {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        1 | 2 if can_write => SeededDemoOperation::SendViaAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        3 if can_write => SeededDemoOperation::SendViaHttpAction {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        4 if can_write => SeededDemoOperation::ScheduleSend {
            author: seeded_convex_demo_author(state),
            body: seeded_convex_demo_body(seed, step_index, state),
        },
        5 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(4) {
                None
            } else {
                Some(
                    created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                        .author
                        .clone(),
                )
            };
            SeededDemoOperation::QueryByAuthor { author }
        }
        6 => SeededDemoOperation::LoadViaHttpAction {
            author: created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                .author
                .clone(),
        },
        7 => SeededDemoOperation::LoadById {
            message_index: (seeded_convex_demo_draw(state) as usize) % created.len(),
        },
        8 => {
            let author = if seeded_convex_demo_draw(state).is_multiple_of(5) {
                format!("missing-author-{}", step_index)
            } else {
                created[(seeded_convex_demo_draw(state) as usize) % created.len()]
                    .author
                    .clone()
            };
            SeededDemoOperation::CheckUnique { author }
        }
        _ => {
            if seeded_convex_demo_draw(state).is_multiple_of(2) {
                let message = &created[(seeded_convex_demo_draw(state) as usize) % created.len()];
                SeededDemoOperation::CheckExact {
                    author: message.author.clone(),
                    body: message.body.clone(),
                    expect_match: true,
                }
            } else {
                SeededDemoOperation::CheckExact {
                    author: seeded_convex_demo_author(state),
                    body: format!("missing-body-{}", step_index),
                    expect_match: false,
                }
            }
        }
    }
}

fn assert_messages_match_expected(
    actual: &serde_json::Value,
    expected: &[CreatedMessage],
    author: Option<&str>,
    context: &str,
) {
    let actual_messages = normalize_message_snapshots(
        actual
            .as_array()
            .expect("messages response should contain an array"),
    );
    assert_eq!(
        actual_messages,
        expected_message_snapshots(expected, author),
        "{context}"
    );
}

async fn execute_faulted_seeded_convex_demo_overlap<F>(
    api: &HttpApiFixture<'_>,
    server: &ServerFixture,
    faults: &std::sync::Arc<ArmedBlockingFaultInjector>,
    created: &mut Vec<CreatedMessage>,
    seed: u64,
    step_index: usize,
    context: &F,
) where
    F: Fn(&str, Option<usize>) -> String,
{
    let author = format!("faulted-seed-{seed}");
    let action_body = format!("faulted-action-{step_index}");
    let http_body = format!("faulted-http-{step_index}");
    let second_action_body = format!("faulted-follow-up-{step_index}");

    faults.arm();

    let mut action = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/action");
        let author = author.clone();
        let action_body = action_body.clone();
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:sendViaAction",
                    "args": { "author": author, "body": action_body }
                }))
                .send()
                .await
                .expect("runtime-backed action should resolve")
        }
    });

    timeout(
        seeded_convex_demo_request_timeout(),
        faults.wait_until_entered(),
    )
    .await
    .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut action)
            .await
            .is_err(),
        "{}",
        context(
            "faulted seeded action should remain pending while apply is blocked",
            Some(step_index),
        )
    );

    let mut blocked_query = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/query");
        let author = author.clone();
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:byAuthor",
                    "args": { "author": author }
                }))
                .send()
                .await
                .expect("blocked query should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "{}",
        context(
            "faulted seeded query should remain pending until apply resumes",
            Some(step_index),
        )
    );

    let mut http_post = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_http_url("demo", "/messages");
        let author = author.clone();
        let http_body = http_body.clone();
        async move {
            client
                .post(url)
                .json(&json!({ "author": author, "body": http_body }))
                .send()
                .await
                .expect("httpAction post should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut http_post)
            .await
            .is_err(),
        "{}",
        context(
            "faulted seeded httpAction post should remain pending while apply is blocked",
            Some(step_index),
        )
    );

    faults.release();

    let action = timeout(seeded_convex_demo_request_timeout(), action)
        .await
        .expect("runtime-backed action should resolve after apply resumes")
        .expect("action task should join");
    assert_eq!(
        action.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded action should succeed after apply resumes",
            Some(step_index),
        )
    );
    let action_id = action
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded action response should parse");
    let action_message = wait_for_message_record(api, &author, &action_body).await;
    assert_eq!(
        action_id,
        json!(action_message.id),
        "{}",
        context(
            "faulted seeded action should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(action_message);

    let blocked_query = timeout(seeded_convex_demo_request_timeout(), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join");
    assert_eq!(
        blocked_query.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded query should succeed after apply resumes",
            Some(step_index),
        )
    );
    let blocked_query_body = blocked_query
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded query response should parse");
    assert!(blocked_query_body.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(author) && message["body"] == json!(action_body)
        })
    }));

    let http_post = timeout(seeded_convex_demo_request_timeout(), &mut http_post)
        .await
        .expect("follow-up httpAction post should resolve after apply resumes")
        .expect("httpAction post task should join");
    assert_eq!(
        http_post.status(),
        StatusCode::CREATED,
        "{}",
        context(
            "faulted seeded httpAction post should succeed after apply resumes",
            Some(step_index),
        )
    );
    let http_post_body = http_post
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded httpAction post response should parse");
    let http_message = wait_for_message_record(api, &author, &http_body).await;
    assert_eq!(
        http_post_body["id"],
        json!(http_message.id),
        "{}",
        context(
            "faulted seeded httpAction post should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(http_message);

    let second_action = timeout(
        seeded_convex_demo_request_timeout(),
        api.convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": author, "body": second_action_body }),
        ),
    )
    .await
    .expect("follow-up runtime-backed action should resolve after the faulted overlap");
    assert_eq!(
        second_action.status(),
        StatusCode::OK,
        "{}",
        context(
            "faulted seeded follow-up action should succeed after overlap recovery",
            Some(step_index),
        )
    );
    let second_action_id = second_action
        .json::<serde_json::Value>()
        .await
        .expect("faulted seeded follow-up action response should parse");
    let second_action_message = wait_for_message_record(api, &author, &second_action_body).await;
    assert_eq!(
        second_action_id,
        json!(second_action_message.id),
        "{}",
        context(
            "faulted seeded follow-up action should return the inserted message id",
            Some(step_index),
        )
    );
    created.push(second_action_message);
}

async fn assert_seeded_convex_demo_usage_scenario_matches_model(
    seed: u64,
    operation_count: usize,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
    faulted_overlap_step: Option<usize>,
) {
    let registry = http_demo_registry(0);
    let (fixture, faults) = if faulted_overlap_step.is_some() {
        let faults = ArmedBlockingFaultInjector::new(
            neovex_storage::FaultPoint::JournalDurableAppendBeforeApply,
        );
        let harness = DeterministicHarness::with_fault_injector(
            ScenarioMetadata::new(
                format!("{test_name}-faulted-overlap"),
                seed.saturating_add(10_000),
            ),
            Arc::new(neovex_storage::ManualClock::new(neovex_core::Timestamp(
                seed.saturating_add(10_000),
            ))),
            faults.clone(),
        );
        let fixture = ServiceFixture::new_with_harness(harness, |path, harness| {
            Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
        });
        (fixture, Some(faults))
    } else {
        (ServiceFixture::new(|path| Service::new(path)), None)
    };
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service, shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let context = |invariant: &str, step_index: Option<usize>| {
        seeded_convex_demo_context(
            seed,
            operation_count,
            case,
            test_name,
            invariant,
            step_index,
        )
    };

    let mut state = seed;
    let mut created = Vec::new();

    for step_index in 0..operation_count {
        if faulted_overlap_step == Some(step_index) {
            execute_faulted_seeded_convex_demo_overlap(
                &api,
                &server,
                faults
                    .as_ref()
                    .expect("faulted overlap steps require a blocking fault injector"),
                &mut created,
                seed,
                step_index,
                &context,
            )
            .await;
            continue;
        }

        match choose_seeded_convex_demo_operation(seed, step_index, &created, &mut state) {
            SeededDemoOperation::SendViaAction { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_action(
                        "demo",
                        "messages:sendViaAction",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{}",
                        context(
                            &format!(
                                "seeded action should resolve for author {author} body {body}"
                            ),
                            Some(step_index),
                        )
                    )
                });
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded action write should succeed", Some(step_index))
                );
                let returned_id = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("action response should parse");
                let message = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_id,
                    json!(message.id),
                    "{}",
                    context(
                        "action responses should return the inserted message id",
                        Some(step_index),
                    )
                );
                created.push(message);
            }
            SeededDemoOperation::SendViaHttpAction { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_http_json(
                        "demo",
                        reqwest::Method::POST,
                        "/messages",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("httpAction post should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::CREATED,
                    "{}",
                    context("seeded httpAction post should succeed", Some(step_index))
                );
                let returned_body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("httpAction post response should parse");
                let message = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_body["id"],
                    json!(message.id),
                    "{}",
                    context(
                        "httpAction post responses should return the inserted message id",
                        Some(step_index),
                    )
                );
                created.push(message);
            }
            SeededDemoOperation::ScheduleSend { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_mutation(
                        "demo",
                        "messages:scheduleSend",
                        json!({ "author": author, "body": body, "delayMs": 0 }),
                    ),
                )
                .await
                .expect("scheduled mutation should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded scheduled mutation should succeed", Some(step_index))
                );
                let message = wait_for_message_record(&api, &author, &body).await;
                created.push(message);
            }
            SeededDemoOperation::RuntimeSendAndSchedule { author, body } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_mutation(
                        "demo",
                        "messages:sendAndSchedule",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("runtime mutation should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded runtime mutation should succeed", Some(step_index))
                );
                let returned_id = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("runtime mutation response should parse");
                let immediate = wait_for_message_record(&api, &author, &body).await;
                assert_eq!(
                    returned_id,
                    json!(immediate.id),
                    "{}",
                    context(
                        "runtime mutation responses should return the immediate message id",
                        Some(step_index),
                    )
                );
                created.push(immediate);
                let scheduled =
                    wait_for_message_record(&api, &author, &format!("{body} (scheduled)")).await;
                created.push(scheduled);
            }
            SeededDemoOperation::QueryByAuthor { author } => {
                let messages = query_messages_by_author(&api, author.as_deref()).await;
                assert_messages_match_expected(
                    &messages,
                    &created,
                    author.as_deref(),
                    &context(
                        "seeded query should match the expected message set",
                        Some(step_index),
                    ),
                );
            }
            SeededDemoOperation::LoadViaHttpAction { author } => {
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_http(
                        "demo",
                        reqwest::Method::GET,
                        &format!("/messages/by-author?author={author}"),
                    ),
                )
                .await
                .expect("httpAction get should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded httpAction get should succeed", Some(step_index))
                );
                let messages = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("httpAction get response should parse");
                assert_messages_match_expected(
                    &messages,
                    &created,
                    Some(&author),
                    &context(
                        "seeded httpAction get should match the expected message set",
                        Some(step_index),
                    ),
                );
            }
            SeededDemoOperation::LoadById { message_index } => {
                let message = &created[message_index];
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query("demo", "messages:byId", json!({ "id": message.id })),
                )
                .await
                .expect("byId query should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("seeded byId query should succeed", Some(step_index))
                );
                let body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("byId query response should parse");
                assert_eq!(body["author"], json!(message.author));
                assert_eq!(body["body"], json!(message.body));
            }
            SeededDemoOperation::CheckUnique { author } => {
                let expected_matches = created
                    .iter()
                    .filter(|message| message.author == author)
                    .collect::<Vec<_>>();
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query(
                        "demo",
                        "messages:uniqueByAuthor",
                        json!({ "author": author }),
                    ),
                )
                .await
                .expect("unique query should resolve");
                match expected_matches.as_slice() {
                    [] => {
                        assert_eq!(
                            response.status(),
                            StatusCode::OK,
                            "{}",
                            context(
                                "unique query with no matching author should succeed",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query response should parse");
                        assert_eq!(
                            body,
                            serde_json::Value::Null,
                            "{}",
                            context(
                                "unique query without a match should return null",
                                Some(step_index)
                            )
                        );
                    }
                    [message] => {
                        assert_eq!(
                            response.status(),
                            StatusCode::OK,
                            "{}",
                            context(
                                "unique query with one matching author should succeed",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query response should parse");
                        assert_eq!(body["author"], json!(message.author));
                        assert_eq!(body["body"], json!(message.body));
                    }
                    _ => {
                        assert_eq!(
                            response.status(),
                            StatusCode::BAD_REQUEST,
                            "{}",
                            context(
                                "unique query with duplicate matches should fail",
                                Some(step_index)
                            )
                        );
                        let body = response
                            .json::<serde_json::Value>()
                            .await
                            .expect("unique query error should parse");
                        assert!(
                            body["error"]
                                .as_str()
                                .is_some_and(|message| message.contains("multiple documents")),
                            "{}",
                            context(
                                "duplicate unique query errors should explain the multiple-document conflict",
                                Some(step_index),
                            )
                        );
                    }
                }
            }
            SeededDemoOperation::CheckExact {
                author,
                body,
                expect_match,
            } => {
                let expected = created
                    .iter()
                    .find(|message| message.author == author && message.body == body);
                let response = timeout(
                    seeded_convex_demo_request_timeout(),
                    api.convex_named_query(
                        "demo",
                        "messages:exactByAuthorAndBody",
                        json!({ "author": author, "body": body }),
                    ),
                )
                .await
                .expect("exact query should resolve");
                assert_eq!(
                    response.status(),
                    StatusCode::OK,
                    "{}",
                    context("exact query should succeed", Some(step_index))
                );
                let response_body = response
                    .json::<serde_json::Value>()
                    .await
                    .expect("exact query response should parse");
                match expected {
                    Some(message) => {
                        assert!(
                            expect_match,
                            "{}",
                            context(
                                "exact-match scenarios should only be generated when the oracle expects a message",
                                Some(step_index),
                            )
                        );
                        assert_eq!(response_body["author"], json!(message.author));
                        assert_eq!(response_body["body"], json!(message.body));
                    }
                    None => {
                        assert!(
                            !expect_match,
                            "{}",
                            context(
                                "missing exact-match scenarios should only be generated when the oracle expects null",
                                Some(step_index),
                            )
                        );
                        assert_eq!(
                            response_body,
                            serde_json::Value::Null,
                            "{}",
                            context("missing exact queries should return null", Some(step_index))
                        );
                    }
                }
            }
        }
    }

    let all_messages = query_messages_by_author(&api, None).await;
    assert_messages_match_expected(
        &all_messages,
        &created,
        None,
        &context(
            "final seeded Convex demo query should match the accumulated message model",
            None,
        ),
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_http_demo_flow_matches_generated_app_behavior() {
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
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
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
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

#[tokio::test]
async fn convex_http_demo_faulted_overlap_still_completes_http_post_and_follow_up_action() {
    let faults = neovex_test_support::BlockingFaultInjector::new(
        neovex_storage::FaultPoint::JournalDurableAppendBeforeApply,
    );
    let harness = DeterministicHarness::with_fault_injector(
        ScenarioMetadata::new("convex-http-demo-faulted-overlap", 61),
        Arc::new(neovex_storage::ManualClock::new(neovex_core::Timestamp(
            61_000,
        ))),
        faults.clone(),
    );
    let registry = http_demo_registry(1_000);
    let fixture = ServiceFixture::new_with_harness(harness, |path, harness| {
        Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let author = "faulted-http-demo";
    let action_body = "first-action";
    let http_body = "follow-up-http";
    let second_action_body = "follow-up-action";

    let mut action = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/action");
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:sendViaAction",
                    "args": { "author": author, "body": action_body }
                }))
                .send()
                .await
                .expect("runtime-backed action should resolve")
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut action)
            .await
            .is_err(),
        "blocked runtime-backed action should remain pending until apply resumes"
    );

    let mut blocked_query = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_url("demo", "/query");
        async move {
            client
                .post(url)
                .json(&json!({
                    "name": "messages:byAuthor",
                    "args": { "author": author }
                }))
                .send()
                .await
                .expect("blocked query should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut http_post = tokio::spawn({
        let client = server.client().clone();
        let url = api.convex_http_url("demo", "/messages");
        async move {
            client
                .post(url)
                .json(&json!({ "author": author, "body": http_body }))
                .send()
                .await
                .expect("httpAction post should resolve")
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut http_post)
            .await
            .is_err(),
        "follow-up httpAction post should remain pending while apply is blocked"
    );

    faults.release();

    let action = timeout(Duration::from_secs(1), action)
        .await
        .expect("runtime-backed action should resolve after apply resumes")
        .expect("action task should join");
    assert_eq!(action.status(), StatusCode::OK);

    let blocked_query = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join");
    assert_eq!(blocked_query.status(), StatusCode::OK);
    let blocked_query_body = blocked_query
        .json::<serde_json::Value>()
        .await
        .expect("blocked query response should parse");
    assert!(blocked_query_body.as_array().is_some_and(|items| {
        items.iter().any(|message| {
            message["author"] == json!(author) && message["body"] == json!(action_body)
        })
    }));

    let http_post = timeout(Duration::from_secs(1), &mut http_post)
        .await
        .expect("follow-up httpAction post should resolve after apply resumes")
        .expect("httpAction post task should join");
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
    .expect("follow-up runtime-backed action should resolve after the faulted overlap");
    assert_eq!(second_action.status(), StatusCode::OK);
    wait_for_message(&api, author, second_action_body).await;
}

#[tokio::test]
async fn convex_http_demo_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        17,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_seeded_usage_scenario_matches_model",
        None,
    )
    .await;
}

#[tokio::test]
async fn convex_http_demo_faulted_seeded_usage_scenario_matches_model() {
    assert_seeded_convex_demo_usage_scenario_matches_model(
        23,
        seeded_convex_demo_operation_count(24),
        None,
        "convex_http_demo_faulted_seeded_usage_scenario_matches_model",
        Some(seeded_convex_demo_faulted_overlap_step(
            seeded_convex_demo_operation_count(24),
        )),
    )
    .await;
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            seeded_convex_demo_operation_count(case.step_count),
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
            None,
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            seeded_convex_demo_operation_count(case.step_count),
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
            None,
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let operation_count = seeded_convex_demo_operation_count(case.step_count);
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            operation_count,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
            Some(seeded_convex_demo_faulted_overlap_step(operation_count)),
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model_on_faulted_convex_demo_surface()
 {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let operation_count = seeded_convex_demo_operation_count(case.step_count);
        assert_seeded_convex_demo_usage_scenario_matches_model(
            case.seed,
            operation_count,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
            Some(seeded_convex_demo_faulted_overlap_step(operation_count)),
        )
        .await;
    }
}
