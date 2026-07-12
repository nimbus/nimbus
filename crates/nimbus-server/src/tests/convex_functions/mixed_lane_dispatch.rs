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

// The nested ctx.run* dispatcher resolves each callee's lane HOST-side against
// this app's registry (op_nimbus_ctx_resolve_callee_lane), so this bundle
// publishes no callee-lane lookup or registrar — there is no guest-reachable
// state to tamper with. The `definitions` map above is used only by the
// visibility gate below, mirroring what generated bundles carry.

// Generated-bundle parity (invokeNamedDefinitionLocally in
// runtime_bundle_dispatch_invocation.mjs): both bundle entry points gate on
// the caller's reference visibility exactly the way emitted bundles do, so
// this harness reproduces what a real codegen app observes. Keep this in
// sync with the emitted gate: an explicit request.visibility is the
// reference tree of a same-isolate nested ctx.run* call and must match the
// definition; a host-constructed request (client traffic or a cross-lane
// nested call re-entering through host dispatch) omits it because the host
// already enforced visibility.
const assertReferenceVisibility = (request) => {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return;
  }
  if (
    typeof request.visibility === "string"
    && definition.visibility !== request.visibility
  ) {
    throw new Error(
      "nimbus function "
        + request.function_name
        + " is "
        + definition.visibility
        + ", not "
        + request.visibility,
    );
  }
};

async function invokeNamedLocal(request) {
  const handler = handlers[request.function_name];
  if (!handler) {
    throw new Error("missing local handler: " + request.function_name);
  }
  assertReferenceVisibility(request);
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: request.hostCallSessionId,
    invokeNamedLocal,
  });
  return await handler(ctx, request.args ?? {});
}

globalThis.__nimbusInvoke = async function (request) {
  try {
    const handler = handlers[request.function_name];
    if (!handler) {
      return {
        status: "error",
        error: { kind: "internal", message: "missing handler " + request.function_name },
      };
    }
    assertReferenceVisibility(request);
    const ctx = globalThis.__nimbusCreateContext({ request, invokeNamedLocal });
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

/// A module-top-level tampering preamble that runs before any handler — the
/// exact timing an eagerly-imported npm/builtin dependency evaluates at. It
/// attempts every lane-routing attack the removed JS mechanism was vulnerable
/// to: the bare realm-global identifier reassignment the review found, the older
/// global, and pre-registering an always-"same lane" impostor with the old
/// registrar. After the fix none of these exist or are consulted (the callee
/// lane is resolved host-side), so cross-lane routing is unaffected.
const CONVEX_MIXED_LANE_TAMPER_PREAMBLE: &str = r#"
try { __nimbusCapturedCalleeLaneLookup = () => globalThis.__nimbusRuntimeEnvironmentLane; } catch (_e) {}
try { globalThis.__nimbusLocalFunctionRuntimeEnvironment = () => globalThis.__nimbusRuntimeEnvironmentLane; } catch (_e) {}
try {
  if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
    globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(() => globalThis.__nimbusRuntimeEnvironmentLane);
  }
} catch (_e) {}
"#;

fn build_convex_mixed_lane_app() -> ConvexMixedLaneApp {
    build_convex_mixed_lane_app_with_preamble("")
}

fn build_convex_mixed_lane_app_with_preamble(bundle_preamble: &str) -> ConvexMixedLaneApp {
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
    let bundle_source = format!("{bundle_preamble}{CONVEX_MIXED_LANE_BUNDLE}");
    fs::write(&bundle_path, bundle_source.as_bytes())
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
    assert_cross_lane_execution_on_callee_lane(build_convex_mixed_lane_app()).await;
}

/// Guest tampering at module-evaluation time — the timing an eagerly-imported
/// dependency runs at (EX10R3.1 second vector) — must not influence the
/// local-vs-host lane decision. The callee lane is resolved host-side, so both
/// cross-lane directions still execute on the callee's own lane exactly as they
/// do without the attack.
#[tokio::test]
async fn convex_cross_lane_dispatch_survives_module_scope_lane_tampering() {
    assert_cross_lane_execution_on_callee_lane(build_convex_mixed_lane_app_with_preamble(
        CONVEX_MIXED_LANE_TAMPER_PREAMBLE,
    ))
    .await;
}

async fn assert_cross_lane_execution_on_callee_lane(app: ConvexMixedLaneApp) {
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

/// The relaxed bundle-side gate (host-constructed requests carry no reference
/// visibility) must not open internal functions to clients: the client-facing
/// boundary is host-side registry resolution, which rejects an internal
/// target before any runtime dispatch happens.
#[tokio::test]
async fn convex_client_calls_to_internal_functions_stay_rejected() {
    let app = build_convex_mixed_lane_app();
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(service, app.registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    // Internal default-lane mutation: exactly the function the node action is
    // allowed to reach through nested host dispatch.
    let response = api
        .convex_named_mutation("demo", "clock:probe", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("internal mutation rejection should parse");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("internal mutation rejection should carry a message")
            .contains("not public"),
        "{body}"
    );

    // Internal "use node" action: same rejection on the action route.
    let response = api
        .convex_named_action("demo", "nodeside:digest", json!({ "body": "x" }))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("internal action rejection should parse");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("internal action rejection should carry a message")
            .contains("not public"),
        "{body}"
    );
}
