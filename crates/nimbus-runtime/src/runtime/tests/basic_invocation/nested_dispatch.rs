//! Nested ctx.run* lane routing: same-isolate local dispatch is only taken when
//! the callee's runtime lane matches the lane the current isolate executes;
//! cross-lane calls go through host dispatch (the engine path), which resolves
//! the callee's own lane and semantics profile.
//!
//! The local-vs-host decision is resolved HOST-side
//! (`op_nimbus_ctx_resolve_callee_lane`): the runtime asks the host for the
//! callee's authoritative lane and compares it against this isolate's frozen
//! lane. There is deliberately no guest-reachable JavaScript lane lookup or
//! registrar, so no handler body or eagerly-imported dependency can influence
//! the decision. These tests exercise the honest routing paths and the
//! adversarial tampering the removed JS mechanism used to be vulnerable to.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;

use super::support::*;
use super::*;
use crate::error::{NimbusRuntimeError, Result};
use crate::host::{HostBridge, HostCallOperation, HostCallRequest};

/// Test host that answers the callee-lane oracle from an explicit name→lane
/// map (standing in for the server registry) and records every host call so the
/// tests can assert which dispatch path a nested call took. Unknown callees
/// resolve to JSON null, exactly as the real registry reports a function it does
/// not own — the runtime must then fail safe to host dispatch.
struct LaneOracleHost {
    lanes: HashMap<String, String>,
    calls: Mutex<Vec<HostCallRequest>>,
}

impl LaneOracleHost {
    fn new(lanes: &[(&str, &str)]) -> Self {
        Self {
            lanes: lanes
                .iter()
                .map(|(name, lane)| ((*name).to_string(), (*lane).to_string()))
                .collect(),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn operations(&self) -> Vec<HostCallOperation> {
        self.calls
            .lock()
            .expect("lane oracle host lock should not be poisoned")
            .iter()
            .map(|call| call.operation)
            .collect()
    }
}

impl HostBridge for LaneOracleHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        self.calls
            .lock()
            .expect("lane oracle host lock should not be poisoned")
            .push(request.clone());
        match request.operation {
            HostCallOperation::CtxResolveCalleeLane => {
                let name = request
                    .payload
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        NimbusRuntimeError::Contract(
                            "callee-lane oracle payload is missing `name`".to_string(),
                        )
                    })?;
                let value = self
                    .lanes
                    .get(name)
                    .map(|lane| Value::String(lane.clone()))
                    .unwrap_or(Value::Null);
                Ok(serde_json::json!({ "status": "ok", "value": value }))
            }
            _ => Ok(serde_json::json!({
                "operation": request.operation,
                "payload": request.payload,
            })),
        }
    }
}

/// A generated-bundle-shaped module: the local dispatcher plus a caller that
/// runs the nested call named by `args.target` with the kind in
/// `args.nested_kind`. The bundle publishes no lane lookup — the host owns that.
const LANE_ROUTING_BUNDLE: &str = r#"
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

/// Same shape as [`LANE_ROUTING_BUNDLE`], but the caller runs adversarial guest
/// tampering before the nested call. It simulates a `new Function`-compiled
/// handler body (sloppy mode, the environment real guest handlers run in) that:
///   * reassigns the bare realm-global identifier the review found —
///     `__nimbusCapturedCalleeLaneLookup = () => currentLane` — to claim every
///     callee shares this isolate's lane,
///   * reassigns the historical `__nimbusLocalFunctionRuntimeEnvironment`
///     global, and
///   * tries to (re-)register an always-"same lane" impostor through the old
///     registrar.
///
/// After the fix none of these exist or are consulted: the callee lane is
/// resolved host-side, so a cross-lane nested call still routes to the host.
// A second-layer attack: the callee lane is resolved host-side, but the guest
// reassigns the sync host-call transport global itself to forge the oracle's
// answer for `op_nimbus_ctx_resolve_callee_lane` (returning this isolate's own
// lane so every callee looks same-lane). Pre-fix the transport global was a
// plain writable assignment, so the dispatcher's dynamic `globalThis.__nimbus
// SyncHostValue(...)` deref hit the impostor; post-fix the property is frozen
// non-writable, the reassignment is inert, and the real host answer wins.
const TRANSPORT_HIJACK_TAMPER_BUNDLE: &str = r#"
globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  const attack = new Function(
    "try {\n" +
    "  const real = globalThis.__nimbusSyncHostValue;\n" +
    "  globalThis.__nimbusSyncHostValue = function (opName, payload) {\n" +
    "    if (opName === 'op_nimbus_ctx_resolve_callee_lane') {\n" +
    "      return globalThis.__nimbusRuntimeEnvironmentLane;\n" +
    "    }\n" +
    "    return real(opName, payload);\n" +
    "  };\n" +
    "} catch (_e) {}\n"
  );
  attack();
  const ctx = globalThis.__nimbusCreateContext({ request });
  const nested = await ctx.runQuery({ name: request.args.target, visibility: "public" }, {});
  return {
    currentLane: globalThis.__nimbusRuntimeEnvironmentLane ?? null,
    nested,
  };
};

export {};
"#;

const LANE_ROUTING_TAMPER_BUNDLE: &str = r#"
globalThis.__nimbusInvokeNamedLocal = async function (request) {
  return { dispatched: "local", name: request.function_name };
};

globalThis.__nimbusInvoke = async function (request) {
  // A sloppy-mode handler body: `new Function` bodies are non-strict and resolve
  // free identifiers against the realm global env, so this is exactly the reach
  // a real guest handler has. Pre-fix, the bare assignment reattached an
  // always-"same lane" impostor to the lexical binding the dispatcher read.
  const attack = new Function(
    "try { __nimbusCapturedCalleeLaneLookup = () => globalThis.__nimbusRuntimeEnvironmentLane; } catch (_e) {}\n" +
    "try { globalThis.__nimbusLocalFunctionRuntimeEnvironment = () => globalThis.__nimbusRuntimeEnvironmentLane; } catch (_e) {}\n" +
    "if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === 'function') {\n" +
    "  try { globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(() => globalThis.__nimbusRuntimeEnvironmentLane); } catch (_e) {}\n" +
    "}\n"
  );
  attack();
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
    lanes: &[(&str, &str)],
) -> (Value, Vec<HostCallOperation>) {
    let (_tempdir, bundle_path) = write_app_style_bundle(bundle_source);
    let host = Arc::new(LaneOracleHost::new(lanes));
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(&RuntimeBundle::new(&bundle_path), request, "tenant-a")
        .await
        .expect("lane-routing bundle invocation should succeed");
    (result, host.operations())
}

fn convex_default_lane_limits() -> RuntimeLimits {
    RuntimeLimits {
        guest_semantics: crate::RuntimeGuestSemantics::ConvexDefault,
        ..run_to_completion_snapshot_runtime_test_limits()
    }
}

/// Every real deployment's isolate freezes a lane and its callees resolve to a
/// lane through the host; both are used verbatim here.
const APP_LANES: &[(&str, &str)] = &[("child:defaultLane", "default"), ("child:nodeLane", "node")];

#[tokio::test]
async fn nested_call_same_lane_stays_on_local_dispatch_in_default_isolate() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:defaultLane", "query"),
        APP_LANES,
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_eq!(
        result["nested"]["dispatched"], "local",
        "a default-lane callee in a default isolate must use local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxResolveCalleeLane),
        "the local-vs-host decision must consult the host oracle: {operations:?}"
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
        APP_LANES,
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
        APP_LANES,
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
        APP_LANES,
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
async fn nested_call_to_unknown_callee_routes_through_host_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // A callee the host does not resolve (null lane) may still be a
    // registry-native function; the host resolves it, the bundle cannot. The
    // runtime must fail safe to host dispatch — never assume local.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:unknown", "query"),
        APP_LANES,
    )
    .await;

    assert_ne!(result["nested"]["dispatched"], "local");
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "unknown callees must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "unknown callees must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn guest_bare_identifier_tampering_cannot_force_cross_lane_local_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // The blocker vector (EX10R3.1): a sloppy-mode handler body reassigns the
    // bare realm-global lookup identifier (and the older globals/registrar) to
    // claim every callee shares this isolate's lane. Pre-fix the dispatcher read
    // that guest-writable state and ran a "use node" callee inside the web
    // isolate. Post-fix the callee lane is resolved host-side, so the cross-lane
    // call still routes to the host regardless of any guest tampering.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:nodeLane", "query"),
        APP_LANES,
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "guest tampering must not force a node callee onto local dispatch: {result}"
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
async fn guest_transport_hijack_of_sync_host_value_cannot_force_cross_lane_local_dispatch() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // Second-layer vector: even with the callee lane resolved host-side, the
    // host answer travels back through `globalThis.__nimbusSyncHostValue`. A
    // guest handler reassigns that transport to forge an always-"same lane"
    // reply for `op_nimbus_ctx_resolve_callee_lane`. The transport globals must
    // be frozen (non-writable) so the impostor never lands and the real host
    // lane still routes a cross-lane callee through host dispatch.
    let (result, operations) = invoke_lane_routing_bundle(
        TRANSPORT_HIJACK_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:nodeLane", "query"),
        APP_LANES,
    )
    .await;

    assert_eq!(result["currentLane"], "default");
    assert_ne!(
        result["nested"]["dispatched"], "local",
        "transport hijack must not force a node callee onto local dispatch: {result}"
    );
    assert!(
        operations.contains(&HostCallOperation::CtxRunQuery),
        "post-hijack cross-lane call must go through host dispatch: {operations:?}"
    );
    assert!(
        !operations.contains(&HostCallOperation::CtxRuntimeEnterNestedCall),
        "post-hijack cross-lane call must not enter the local-dispatch protocol: {operations:?}"
    );
}

#[tokio::test]
async fn guest_tampering_leaves_same_lane_local_dispatch_intact() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // The fix must not over-correct: a genuine same-lane callee still takes the
    // local-dispatch optimization even after the guest tampered, because the
    // host oracle still reports the real lane.
    let (result, operations) = invoke_lane_routing_bundle(
        LANE_ROUTING_TAMPER_BUNDLE,
        convex_default_lane_limits(),
        &caller_request("child:defaultLane", "query"),
        APP_LANES,
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
