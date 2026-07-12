//! Nested ctx.run* lane routing: same-isolate local dispatch is only taken
//! when the callee's manifest `runtime_environment` matches the lane the
//! current isolate executes; cross-lane calls go through host dispatch (the
//! engine path), which resolves the callee's own lane and semantics profile.

use super::support::*;
use super::*;
use crate::host::HostCallOperation;

/// A generated-bundle-shaped module: local dispatcher plus the per-function
/// lane lookup emitted by @nimbus/codegen. The caller is invoked as an
/// action and runs the nested call named by `args.target` with the kind in
/// `args.nested_kind`.
const LANE_ROUTING_BUNDLE: &str = r#"
const functionsByName = new Map([
  ["child:defaultLane", { runtime_environment: "default" }],
  ["child:nodeLane", { runtime_environment: "node" }],
]);

const __laneLookup = function (name) {
  const definition = functionsByName.get(name);
  return definition && typeof definition.runtime_environment === "string"
    ? definition.runtime_environment
    : null;
};
// Generated bundles register the callee-lane lookup with the host-owned
// registrar the context contract installs at bootstrap; the contract consults
// that captured reference, never the guest-visible global name. The plain
// global assignment mirrors the pre-registrar shape so this bundle exercises
// both code states.
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(__laneLookup);
}
globalThis.__nimbusLocalFunctionRuntimeEnvironment = __laneLookup;

globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const nested =
    request.args.nested_kind === "mutation"
      ? await ctx.runMutation({ name: request.args.target, visibility: "public" }, {})
      : await ctx.runQuery({ name: request.args.target, visibility: "public" }, {});
  return {
    currentLane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
    nested,
  };
};

export {};
"#;

/// Same generated-bundle shape as [`LANE_ROUTING_BUNDLE`], but the caller runs
/// adversarial guest tampering before the nested call: it deletes and/or
/// reassigns the historical `__nimbusLocalFunctionRuntimeEnvironment` global to
/// an always-"same lane" function, and tries to re-hijack the host registrar.
/// None of that may force a cross-lane callee onto same-isolate local
/// dispatch: the contract consults the host-captured lookup, not the global.
const LANE_ROUTING_TAMPER_BUNDLE: &str = r#"
const functionsByName = new Map([
  ["child:defaultLane", { runtime_environment: "default" }],
  ["child:nodeLane", { runtime_environment: "node" }],
]);

const __laneLookup = function (name) {
  const definition = functionsByName.get(name);
  return definition && typeof definition.runtime_environment === "string"
    ? definition.runtime_environment
    : null;
};
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(__laneLookup);
}
// A pre-registrar (or tampered) global that the fixed contract must ignore.
globalThis.__nimbusLocalFunctionRuntimeEnvironment = __laneLookup;

globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  if (request.args.attack === "delete") {
    delete globalThis.__nimbusLocalFunctionRuntimeEnvironment;
  } else if (request.args.attack === "reassign") {
    globalThis.__nimbusLocalFunctionRuntimeEnvironment = function () {
      // Claim every callee shares this isolate's lane so the pre-fix contract
      // takes local dispatch for a cross-lane callee.
      return globalThis.__nimbusRuntimeEnvironmentLane;
    };
  }
  // Attempt to replace the host-captured lookup outright; the one-shot
  // registrar must reject this.
  let reRegisterRejected = false;
  if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
    try {
      globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(function () {
        return globalThis.__nimbusRuntimeEnvironmentLane;
      });
    } catch (_error) {
      reRegisterRejected = true;
    }
  }
  const ctx = globalThis.__nimbusCreateContext({ request });
  const nested = await ctx.runQuery({ name: request.args.target, visibility: "public" }, {});
  return {
    currentLane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
    reRegisterRejected,
    nested,
  };
};

export {};
"#;

/// A generated bundle that carries a lane (every real isolate does) but never
/// registers a callee-lane lookup — the tampering end state where the lookup is
/// gone. The fixed contract must fail safe to HOST dispatch, never assume local.
const LANE_ROUTING_NO_LOOKUP_BUNDLE: &str = r#"
globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const nested = await ctx.runQuery({ name: request.args.target, visibility: "public" }, {});
  return {
    currentLane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
    nested,
  };
};

export {};
"#;

fn caller_request(target: &str, nested_kind: &str) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Action,
        function_name: "caller:run".to_string(),
        args: serde_json::json!({ "target": target, "nested_kind": nested_kind }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn attack_request(target: &str, attack: &str) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Action,
        function_name: "caller:run".to_string(),
        args: serde_json::json!({ "target": target, "attack": attack }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

async fn invoke_lane_routing_bundle(
    bundle_source: &str,
    limits: RuntimeLimits,
    request: &InvocationRequest,
) -> (Value, Vec<HostCallOperation>) {
    let (_tempdir, bundle_path) = write_app_style_bundle(bundle_source);
    let host = Arc::new(RecordingHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(&RuntimeBundle::new(&bundle_path), request, "tenant-a")
        .await
        .expect("lane-routing bundle invocation should succeed");
    let operations = host
        .calls
        .lock()
        .expect("recording host lock should not be poisoned")
        .iter()
        .map(|call| call.operation)
        .collect();
    (result, operations)
}

fn convex_default_lane_limits() -> RuntimeLimits {
    RuntimeLimits {
        guest_semantics: crate::RuntimeGuestSemantics::ConvexDefault,
        ..run_to_completion_snapshot_runtime_test_limits()
    }
}

#[tokio::test]
async fn nested_call_same_lane_stays_on_local_dispatch_in_default_isolate() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:defaultLane", "query"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_eq!(
        result["nested"]["dispatched"], "local",
        "a default-lane callee in a default isolate must use local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "local dispatch must announce the nested call to the host: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRunQuery),
        "same-lane nested calls must not fall back to host dispatch: {operations:?}"
    );
}

#[tokio::test]
async fn nested_call_to_node_callee_routes_through_host_dispatch_from_default_isolate() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:nodeLane", "query"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "a \"use node\" callee must never run locally inside the web isolate: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "cross-lane nested calls must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "cross-lane nested calls must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn nested_call_to_default_callee_routes_through_host_dispatch_from_node22_isolate() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        RuntimeLimits::application_node22(),
        &caller_request("child:defaultLane", "mutation"),
    )
    .await;

    assert_eq!(result["currentLane"], "node");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "a default-lane mutation must never run under the Node isolate's Host semantics: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunMutation),
        "node-to-default nested calls must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "node-to-default nested calls must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn nested_call_same_lane_stays_on_local_dispatch_in_node22_isolate() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        RuntimeLimits::application_node22(),
        &caller_request("child:nodeLane", "query"),
    )
    .await;

    assert_eq!(result["currentLane"], "node");
    assert_eq!(
        result["nested"]["dispatched"], "local",
        "a node-lane callee in a node isolate must use local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "local dispatch must announce the nested call to the host: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRunQuery),
        "same-lane nested calls must not fall back to host dispatch: {operations:?}"
    );
}

#[tokio::test]
async fn nested_call_with_lane_but_no_lookup_fails_safe_to_host_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // Every real isolate freezes a lane at bootstrap. A bundle that carries a
    // lane but registers no callee-lane lookup — whether a stripped-down
    // harness bundle or the end state of guest tampering that removed the
    // lookup — must NOT fall through to same-isolate local dispatch. The
    // contract fails safe to HOST dispatch, which resolves the callee's own
    // lane and semantics.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_NO_LOOKUP_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:nodeLane", "query"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "a lane-carrying bundle with no callee-lane lookup must fail safe to host dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "missing lookup must route through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "missing lookup must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn guest_delete_of_lookup_global_cannot_force_cross_lane_local_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // Adversarial: the guest deletes the historical lookup global before a
    // cross-lane nested call. Pre-fix the contract re-read that global, found
    // it gone, and fell open to local dispatch — running a "use node" callee
    // inside the web isolate. The fix consults a host-captured reference the
    // delete cannot reach, so the cross-lane call still routes to the host.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &attack_request("child:nodeLane", "delete"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "deleting the lookup global must not force a node callee onto local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "post-tamper cross-lane call must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "post-tamper cross-lane call must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn guest_reassignment_of_lookup_cannot_force_cross_lane_local_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // Adversarial: the guest reassigns the lookup global to a function that
    // claims every callee shares this isolate's lane, and also tries to
    // re-register a hijacked lookup with the host registrar. Neither can
    // redirect lane routing: the registrar is one-shot, and the contract reads
    // the host-captured reference registered at bundle eval.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &attack_request("child:nodeLane", "reassign"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_eq!(
        result["reRegisterRejected"], true,
        "the one-shot registrar must reject a second registration: {result}"
    );
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "reassigning the lookup global must not force a node callee onto local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "post-tamper cross-lane call must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "post-tamper cross-lane call must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn guest_tampering_leaves_same_lane_local_dispatch_intact() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // The fix must not over-correct: a genuine same-lane callee still takes the
    // local-dispatch optimization even after the guest reassigned the global,
    // because the host-captured lookup still reports the real lane.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &attack_request("child:defaultLane", "reassign"),
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_eq!(
        result["nested"]["dispatched"], "local",
        "a same-lane callee must still use local dispatch after tampering: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "same-lane local dispatch must announce the nested call: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRunQuery),
        "same-lane local dispatch must not fall back to host dispatch: {operations:?}"
    );
}

#[tokio::test]
async fn nested_call_to_unknown_callee_routes_through_host_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // A callee missing from the bundle manifest may still be a
    // registry-native function; the host resolves it, the bundle cannot.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:unknown", "query"),
    )
    .await;

    assert_ne!(result["nested"]["dispatched"], "local");
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "unknown callees must go through host dispatch: {operations:?}"
    );
}
