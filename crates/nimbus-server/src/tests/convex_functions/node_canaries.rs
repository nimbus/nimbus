use super::super::*;

struct ConvexUseNodeCanaryApp {
    registry: ConvexRegistry,
    _tempdir: tempfile::TempDir,
}

const CONVEX_USE_NODE_CANARY_BUNDLE: &str = r#"
import { Buffer } from "node:buffer";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";
import { decorate } from "convex-canary-package";

const definitions = new Map([
  ["messages:listByAuthor", {
    name: "messages:listByAuthor",
    kind: "query",
    visibility: "internal",
    plan: {
      table: "messages",
      filters: [
        {
          field: "author",
          op: "eq",
          value: { $arg: "author" },
        },
      ],
      order: { field: "body", direction: "asc" },
      limit: null,
    },
  }],
  ["messages:storeInternal", {
    name: "messages:storeInternal",
    kind: "mutation",
    visibility: "internal",
    plan: {
      type: "insert",
      table: "messages",
      fields: {
        author: { $arg: "author" },
        body: { $arg: "body" },
        source: { $arg: "source" },
        metadata: { $arg: "metadata" },
      },
    },
  }],
  ["messages:scheduledWrite", {
    name: "messages:scheduledWrite",
    kind: "mutation",
    visibility: "internal",
    schedulable: true,
    plan: {
      type: "insert",
      table: "messages",
      fields: {
        author: { $arg: "author" },
        body: { $arg: "body" },
        source: { $arg: "source" },
        metadata: { $arg: "metadata" },
      },
    },
  }],
  ["messages:nodeChildAction", {
    name: "messages:nodeChildAction",
    kind: "action",
    visibility: "internal",
    runtime_handler: "async (_ctx, args, request, helpers) => helpers.nodeChildAction(args, request)",
    plan: null,
  }],
  ["messages:useNodeCanary", {
    name: "messages:useNodeCanary",
    kind: "action",
    visibility: "public",
    runtime_handler: "async (ctx, args, request, helpers) => helpers.useNodeCanary(ctx, args, request)",
    plan: null,
  }],
  ["messages:danglingPromise", {
    name: "messages:danglingPromise",
    kind: "action",
    visibility: "public",
    runtime_handler: "async (_ctx, _args, _request, helpers) => helpers.danglingPromise()",
    plan: null,
  }],
]);

async function readableText(stream) {
  let text = "";
  for await (const chunk of stream) {
    text += chunk;
  }
  return text;
}

async function nodeSurfaceProbe(body) {
  const localDir = path.dirname(new URL(import.meta.url).pathname);
  const canaryFile = path.join(localDir, "convex-use-node-canary.tmp");
  fs.writeFileSync(canaryFile, "tmp-ok", "utf8");

  const response = await fetch(
    "data:application/json,%7B%22source%22%3A%22convex-use-node%22%7D",
  );
  const fetchBody = await response.json();

  return {
    nodeMajor: Number.parseInt(process.versions.node.split(".")[0], 10),
    releaseLts: process.release.lts ?? null,
    packageValue: decorate(body),
    bufferValue: Buffer.from("convex", "utf8").toString("base64"),
    cryptoHash: crypto
      .createHash("sha256")
      .update("nimbus-convex-use-node")
      .digest("hex")
      .slice(0, 12),
    streamText: await readableText(Readable.from(["stream", "-", "ok"])),
    pathBase: path.basename(canaryFile),
    fsTemp: await fs.promises.readFile(canaryFile, "utf8"),
    fetchStatus: response.status,
    fetchBody,
    envSecretBoundary: {
      value: process.env.NIMBUS_CONVEX_CANARY_SECRET ?? null,
      visible: Object.prototype.hasOwnProperty.call(
        process.env,
        "NIMBUS_CONVEX_CANARY_SECRET",
      ),
    },
  };
}

const helpers = {
  async useNodeCanary(ctx, args, request) {
    const before = await ctx.runQuery(
      { name: "messages:listByAuthor", visibility: "internal" },
      { author: args.author },
    );
    const insertedId = await ctx.runMutation(
      { name: "messages:storeInternal", visibility: "internal" },
      {
        author: args.author,
        body: args.body,
        source: "ctx.runMutation",
        metadata: {
          lane: request.services?.lane ?? null,
          nested: false,
        },
      },
    );
    const child = await ctx.runAction(
      { name: "messages:nodeChildAction", visibility: "internal" },
      { body: args.body },
    );
    const scheduledJobId = await ctx.scheduler.runAfter(
      0,
      { name: "messages:scheduledWrite", visibility: "internal" },
      {
        author: args.author,
        body: `${args.body} scheduled`,
        source: "ctx.scheduler",
        metadata: {
          lane: request.services?.lane ?? null,
          scheduled: true,
        },
      },
    );
    const after = await ctx.runQuery(
      { name: "messages:listByAuthor", visibility: "internal" },
      { author: args.author },
    );
    return {
      beforeCount: before.length,
      afterCount: after.length,
      insertedId,
      child,
      scheduledJobId,
      node: await nodeSurfaceProbe(args.body),
      serialization: {
        string: args.body,
        number: 24,
        boolean: true,
        nullValue: null,
        array: ["convex", "use node", args.author],
        object: {
          author: args.author,
          nested: {
            ok: true,
          },
        },
      },
    };
  },
  async nodeChildAction(args) {
    return {
      child: true,
      childNodeMajor: Number.parseInt(process.versions.node.split(".")[0], 10),
      decorated: decorate(args.body),
    };
  },
  danglingPromise() {
    return new Promise(() => {});
  },
};

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "helpers",
    `return (${definition.runtime_handler})(ctx, args, request, helpers);`,
  );
}

const handlers = new Map(
  [...definitions.values()]
    .filter((definition) => typeof definition.runtime_handler === "string")
    .map((definition) => [definition.name, compileRuntimeHandler(definition)]),
);

globalThis.__nimbusInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: {
        kind: "internal",
        message: `missing definition for ${request.function_name}`,
      },
    };
  }
  const handler = handlers.get(definition.name);
  if (!handler) {
    return {
      status: "error",
      error: {
        kind: "internal",
        message: `missing runtime handler for ${request.function_name}`,
      },
    };
  }

  try {
    const value = await handler(
      globalThis.__nimbusCreateContext({
        request,
        hostCallSessionId: `${request.kind}:${request.function_name}`,
      }),
      request.args ?? {},
      request,
      helpers,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

export {};
"#;

fn build_convex_use_node_canary_app(
    target_manifest_value: &str,
    execution_timeout: Duration,
) -> ConvexUseNodeCanaryApp {
    let tempdir = tempdir().expect("convex use-node canary tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    let package_dir = convex_dir
        .join("node_modules")
        .join("convex-canary-package");
    fs::create_dir_all(&package_dir).expect("canary package directory should build");
    fs::write(
        package_dir.join("package.json"),
        r#"{"name":"convex-canary-package","version":"0.0.0","type":"module","main":"index.js"}"#,
    )
    .expect("canary package metadata should write");
    fs::write(
        package_dir.join("index.js"),
        r#"export function decorate(value) { return `pkg:${value}`; }"#,
    )
    .expect("canary package module should write");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:listByAuthor",
                    "kind": "query",
                    "visibility": "internal",
                    "plan": {
                        "table": "messages",
                        "filters": [
                            {
                                "field": "author",
                                "op": "eq",
                                "value": { "$arg": "author" }
                            }
                        ],
                        "order": { "field": "body", "direction": "asc" },
                        "limit": null
                    }
                },
                {
                    "name": "messages:storeInternal",
                    "kind": "mutation",
                    "visibility": "internal",
                    "plan": {
                        "type": "insert",
                        "table": "messages",
                        "fields": {
                            "author": { "$arg": "author" },
                            "body": { "$arg": "body" },
                            "source": { "$arg": "source" },
                            "metadata": { "$arg": "metadata" }
                        }
                    }
                },
                {
                    "name": "messages:scheduledWrite",
                    "kind": "mutation",
                    "visibility": "internal",
                    "schedulable": true,
                    "plan": {
                        "type": "insert",
                        "table": "messages",
                        "fields": {
                            "author": { "$arg": "author" },
                            "body": { "$arg": "body" },
                            "source": { "$arg": "source" },
                            "metadata": { "$arg": "metadata" }
                        }
                    }
                },
                {
                    "name": "messages:nodeChildAction",
                    "kind": "action",
                    "visibility": "internal",
                    "runtime_environment": "node",
                    "runtime_compatibility_target": target_manifest_value,
                    "runtime_package_resolution": "node_external_packages",
                    "runtime_handler": "async (_ctx, args, request, helpers) => helpers.nodeChildAction(args, request)",
                    "plan": null
                },
                {
                    "name": "messages:useNodeCanary",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "node",
                    "runtime_compatibility_target": target_manifest_value,
                    "runtime_package_resolution": "node_external_packages",
                    "runtime_handler": "async (ctx, args, request, helpers) => helpers.useNodeCanary(ctx, args, request)",
                    "plan": null
                },
                {
                    "name": "messages:danglingPromise",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "node",
                    "runtime_compatibility_target": target_manifest_value,
                    "runtime_package_resolution": "node_external_packages",
                    "runtime_handler": "async (_ctx, _args, _request, helpers) => helpers.danglingPromise()",
                    "plan": null
                }
            ]
        }))
        .expect("convex canary functions json should serialize"),
    )
    .expect("convex canary functions manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex canary routes json should serialize"),
    )
    .expect("convex canary routes manifest should write");
    fs::write(
        convex_dir.join("node_external_packages.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "mode": "explicit",
            "configuredExternalPackages": ["convex-canary-package"],
            "stagingRoot": ".nimbus/convex/node_modules",
            "packages": [
                {
                    "packageName": "convex-canary-package",
                    "packageRoot": "node_modules/convex-canary-package",
                    "stagedPackageRoot": ".nimbus/convex/node_modules/convex-canary-package",
                    "sizeBytes": 160,
                    "resolvedSpecifiers": ["convex-canary-package"],
                    "importers": [
                        {
                            "file": "messages.ts",
                            "kind": "import",
                            "specifier": "convex-canary-package"
                        }
                    ]
                }
            ]
        }))
        .expect("node external packages manifest should serialize"),
    )
    .expect("node external packages manifest should write");
    let bundle_path = convex_dir.join("bundle.mjs");
    fs::write(&bundle_path, CONVEX_USE_NODE_CANARY_BUNDLE)
        .expect("convex use-node canary runtime bundle should write");
    let bundle_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    fs::write(
        bundle_path.with_extension("sha256"),
        format!("{bundle_sha256}\n"),
    )
    .expect("convex use-node canary runtime bundle hash should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = execution_timeout;
    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex use-node canary registry should load")
        .with_runtime_limits(limits);
    ConvexUseNodeCanaryApp {
        registry,
        _tempdir: tempdir,
    }
}

async fn run_convex_use_node_real_app_canary(
    target_manifest_value: &str,
    expected_node_major: i64,
) {
    let app = build_convex_use_node_canary_app(target_manifest_value, Duration::from_secs(10));
    let selected_limits = app
        .registry
        .runtime_limits_for_function("messages:useNodeCanary");
    let (lane_executor, lane_policy) = app
        .registry
        .runtime_lane_for_function("messages:useNodeCanary")
        .expect("use-node canary should select an executable runtime lane");
    assert_eq!(
        lane_executor.policy().limits().compatibility_target,
        lane_policy.limits().compatibility_target
    );
    assert_eq!(
        selected_limits
            .compatibility_target
            .node_lts_metadata()
            .expect("use-node canary should select a Node lane")
            .major,
        expected_node_major as u16
    );
    assert!(
        selected_limits.grants.net_connect.is_empty(),
        "Convex use-node production canary must not require broad network grants"
    );

    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(router_for_convex(service, app.registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({
                "author": "Ada",
                "body": "Seed",
                "source": "seed",
                "metadata": { "seed": true }
            })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action(
            "demo",
            "messages:useNodeCanary",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("Convex use-node canary response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["beforeCount"], json!(1));
    assert!(
        body["afterCount"].as_i64().is_some_and(|count| count >= 2),
        "ctx.runQuery after write should observe at least the seed and direct mutation: {body}"
    );
    assert!(body["insertedId"].as_str().is_some());
    assert!(body["scheduledJobId"].as_str().is_some());
    assert_eq!(body["child"]["child"], json!(true));
    assert_eq!(body["child"]["childNodeMajor"], json!(expected_node_major));
    assert_eq!(body["child"]["decorated"], json!("pkg:Hello"));
    assert_eq!(body["node"]["nodeMajor"], json!(expected_node_major));
    assert_eq!(body["node"]["packageValue"], json!("pkg:Hello"));
    assert_eq!(body["node"]["bufferValue"], json!("Y29udmV4"));
    assert_eq!(body["node"]["streamText"], json!("stream-ok"));
    assert_eq!(
        body["node"]["pathBase"],
        json!("convex-use-node-canary.tmp")
    );
    assert_eq!(body["node"]["fsTemp"], json!("tmp-ok"));
    assert_eq!(body["node"]["fetchStatus"], json!(200));
    assert_eq!(
        body["node"]["fetchBody"],
        json!({ "source": "convex-use-node" })
    );
    assert_eq!(
        body["node"]["envSecretBoundary"],
        json!({ "value": null, "visible": false })
    );
    assert_eq!(body["serialization"]["array"][1], json!("use node"));
    assert_eq!(body["serialization"]["object"]["nested"]["ok"], json!(true));
    assert_eq!(
        body["node"]["cryptoHash"]
            .as_str()
            .expect("crypto hash should be a string")
            .len(),
        12
    );

    let documents = timeout(Duration::from_secs(3), async {
        loop {
            let documents = api
                .list_documents("demo", "messages")
                .await
                .json::<serde_json::Value>()
                .await
                .expect("message list should parse");
            if documents["data"].as_array().is_some_and(|documents| {
                documents.iter().any(|document| {
                    document["body"] == json!("Hello scheduled")
                        && document["source"] == json!("ctx.scheduler")
                })
            }) {
                break documents;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Convex use-node canary scheduled mutation should commit");
    assert!(
        documents["data"].as_array().is_some_and(|documents| {
            documents.iter().any(|document| {
                document["body"] == json!("Hello") && document["source"] == json!("ctx.runMutation")
            })
        }),
        "ctx.runMutation write should be present alongside scheduled write: {documents}"
    );

    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
}

async fn run_convex_use_node_dangling_promise_canary(
    target_manifest_value: &str,
    expected_node_major: i64,
) {
    let app = build_convex_use_node_canary_app(target_manifest_value, Duration::from_millis(250));
    let selected_limits = app
        .registry
        .runtime_limits_for_function("messages:danglingPromise");
    assert_eq!(
        selected_limits
            .compatibility_target
            .node_lts_metadata()
            .expect("dangling promise canary should select a Node lane")
            .major,
        expected_node_major as u16
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), app.registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action("demo", "messages:danglingPromise", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("dangling promise diagnostic response should parse");
    let message = body["error"]["message"]
        .as_str()
        .expect("dangling promise diagnostic should include a message");
    assert!(
        message.contains("timed out") || message.contains("pending") || message.contains("Promise"),
        "dangling promise canary should fail with an actionable async diagnostic: {body}"
    );
}

async fn run_convex_use_node_real_app_canary_lane(
    target_manifest_value: &str,
    expected_node_major: i64,
) {
    run_convex_use_node_real_app_canary(target_manifest_value, expected_node_major).await;
    run_convex_use_node_dangling_promise_canary(target_manifest_value, expected_node_major).await;
}

#[tokio::test]
#[ignore = "Convex use-node real app canary: executed by node-compat canary registry"]
async fn convex_use_node_real_app_canary_node22() {
    run_convex_use_node_real_app_canary_lane("22", 22).await;
}

#[tokio::test]
#[ignore = "Convex use-node real app canary: executed by node-compat canary registry"]
async fn convex_use_node_real_app_canary_node24() {
    run_convex_use_node_real_app_canary_lane("24", 24).await;
}

#[tokio::test]
#[ignore = "Convex use-node real app canary: executed by node-compat canary registry"]
async fn convex_use_node_real_app_canary_node26_current() {
    run_convex_use_node_real_app_canary_lane("26", 26).await;
}
