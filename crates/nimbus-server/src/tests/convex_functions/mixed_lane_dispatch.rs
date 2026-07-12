//! Cross-lane nested ctx.run* dispatch through a generated-shaped bundle that
//! is shared between the default web-standard isolate and a "use node"
//! isolate. Both cross-lane directions must leave the isolate through host
//! dispatch and execute on the callee's own lane:
//!
//! - default action -> `ctx.runAction` of a `"use node"` action, which must
//!   reach real node builtins (node:crypto, process.versions) on the Node
//!   lane instead of failing to import inside the web isolate;
//! - node action -> `ctx.runMutation` of a default-lane mutation, which must
//!   run under Convex default-runtime semantics (frozen invocation clock)
//!   instead of the Node isolate's Host semantics.

use super::super::*;

struct ConvexMixedLaneApp {
    registry: ConvexRegistry,
    _tempdir: tempfile::TempDir,
}

const CONVEX_MIXED_LANE_BUNDLE: &str = r#"
const definitions = new Map([
  ["orchestrate:run", { kind: "action", visibility: "public", runtime_environment: "default" }],
  ["nodeside:digest", { kind: "action", visibility: "internal", runtime_environment: "node" }],
  ["clock:probe", { kind: "mutation", visibility: "internal", runtime_environment: "default" }],
]);

const handlers = {
  "orchestrate:run": async (ctx, args) => {
    const digest = await ctx.runAction(
      { name: "nodeside:digest", visibility: "internal" },
      { body: args.body },
    );
    return {
      callerLane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
      digest,
    };
  },
  "nodeside:digest": async (ctx, args) => {
    const { createHash } = await import("node:crypto");
    const clock = await ctx.runMutation(
      { name: "clock:probe", visibility: "internal" },
      {},
    );
    return {
      lane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
      hash: createHash("sha256").update(String(args.body)).digest("hex").slice(0, 12),
      nodeMajor: Number.parseInt(process.versions.node.split(".")[0], 10),
      clock,
    };
  },
  "clock:probe": async () => {
    const first = Date.now();
    await new Promise((resolve) => setTimeout(resolve, 25));
    const second = Date.now();
    return {
      lane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
      first,
      second,
    };
  },
};

globalThis.__nimbusLocalFunctionRuntimeEnvironment = function (name) {
  const definition = definitions.get(name);
  return definition && typeof definition.runtime_environment === "string"
    ? definition.runtime_environment
    : null;
};

globalThis.__nimbusInvokeNamedLocal = async function (request) {
  const handler = handlers[request.function_name];
  if (!handler) {
    throw new Error("missing local handler: " + request.function_name);
  }
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: request.hostCallSessionId,
  });
  return await handler(ctx, request.args ?? {});
};

globalThis.__nimbusInvoke = async function (request) {
  try {
    const handler = handlers[request.function_name];
    if (!handler) {
      return {
        status: "error",
        error: { kind: "internal", message: "missing handler " + request.function_name },
      };
    }
    const ctx = globalThis.__nimbusCreateContext({ request });
    const value = await handler(ctx, request.args ?? {});
    return { status: "ok", value };
  } catch (error) {
    return {
      status: "error",
      error: { kind: "internal", message: String(error?.message ?? error) },
    };
  }
};

export {};
"#;

fn build_convex_mixed_lane_app() -> ConvexMixedLaneApp {
    let tempdir = tempdir().expect("convex mixed-lane tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex dir should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "orchestrate:run",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "default",
                    "runtime_handler": "async (ctx, args) => handlers[\"orchestrate:run\"](ctx, args)",
                    "plan": null
                },
                {
                    "name": "nodeside:digest",
                    "kind": "action",
                    "visibility": "internal",
                    "runtime_environment": "node",
                    "runtime_compatibility_target": "node22",
                    "runtime_handler": "async (ctx, args) => handlers[\"nodeside:digest\"](ctx, args)",
                    "plan": null
                },
                {
                    "name": "clock:probe",
                    "kind": "mutation",
                    "visibility": "internal",
                    "runtime_environment": "default",
                    "runtime_handler": "async (ctx, args) => handlers[\"clock:probe\"](ctx, args)",
                    "plan": null
                }
            ]
        }))
        .expect("mixed-lane functions json should serialize"),
    )
    .expect("mixed-lane functions manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("mixed-lane routes json should serialize"),
    )
    .expect("mixed-lane routes manifest should write");
    let bundle_path = convex_dir.join("bundle.mjs");
    fs::write(&bundle_path, CONVEX_MIXED_LANE_BUNDLE)
        .expect("mixed-lane runtime bundle should write");
    let bundle_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    fs::write(
        bundle_path.with_extension("sha256"),
        format!("{bundle_sha256}\n"),
    )
    .expect("mixed-lane runtime bundle hash should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = Duration::from_secs(30);
    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex mixed-lane registry should load")
        .with_runtime_limits(limits);
    ConvexMixedLaneApp {
        registry,
        _tempdir: tempdir,
    }
}

#[tokio::test]
async fn convex_nested_calls_cross_runtime_lanes_execute_on_the_callee_lane() {
    let app = build_convex_mixed_lane_app();

    // Lane sanity before driving traffic: the registry must place the node
    // action on a Node lane and both default functions on the web lane.
    let node_limits = app.registry.runtime_limits_for_function("nodeside:digest");
    assert_eq!(
        node_limits
            .compatibility_target
            .node_lts_metadata()
            .expect("nodeside:digest should select a Node lane")
            .major,
        22
    );
    let caller_limits = app.registry.runtime_limits_for_function("orchestrate:run");
    assert!(
        caller_limits
            .compatibility_target
            .node_lts_metadata()
            .is_none(),
        "orchestrate:run must stay on the default web lane"
    );

    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(service, app.registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action("demo", "orchestrate:run", json!({ "body": "cross-lane" }))
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("mixed-lane action response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");

    assert_eq!(body["callerLane"], json!("default"), "{body}");

    // Direction 1 (default -> node): the "use node" action really executed on
    // the Node lane and reached node builtins.
    assert_eq!(body["digest"]["lane"], json!("node"), "{body}");
    assert_eq!(body["digest"]["nodeMajor"], json!(22), "{body}");
    // sha256("cross-lane")[..12]
    assert_eq!(body["digest"]["hash"], json!("ff4e58888cb7"), "{body}");

    // Direction 2 (node -> default): the default-lane mutation executed under
    // Convex default-runtime semantics — its invocation clock is frozen for
    // the whole handler, which Host/Node semantics would not do across a
    // 25ms await.
    assert_eq!(body["digest"]["clock"]["lane"], json!("default"), "{body}");
    assert_eq!(
        body["digest"]["clock"]["first"], body["digest"]["clock"]["second"],
        "node->default nested mutation must observe the frozen ConvexDefault clock: {body}"
    );
}
