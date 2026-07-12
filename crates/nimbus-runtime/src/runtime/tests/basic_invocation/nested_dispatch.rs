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

globalThis.__nimbusLocalFunctionRuntimeEnvironment = function (name) {
  const definition = functionsByName.get(name);
  return definition && typeof definition.runtime_environment === "string"
    ? definition.runtime_environment
    : null;
};

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
async fn nested_call_without_lane_metadata_keeps_legacy_local_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // Hand-rolled harness bundles predating lane metadata define only the
    // local dispatcher; they must keep dispatching locally.
    let bundle = r#"
globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const nested = await ctx.runQuery({ name: request.args.target, visibility: "public" }, {});
  return { nested };
};

export {};
"#;
    let (result, operations) = invoke_lane_routing_bundle(
        bundle,
        convex_default_lane_limits(),
        &caller_request("child:nodeLane", "query"),
    )
    .await;

    assert_eq!(result["nested"]["dispatched"], "local");
    assert!(
        operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "legacy bundles keep the local-dispatch protocol: {operations:?}"
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
