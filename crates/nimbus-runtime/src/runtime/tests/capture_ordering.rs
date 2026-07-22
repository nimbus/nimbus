//! Band B-FIX CAPTURE-ORDERING (finding 1): the trusted dispatch entrypoints
//! (`globalThis.__nimbusInvoke` on the node/Convex and Cloud Functions lanes,
//! `globalThis.__nimbusInvokeCloudflareWorkerFetch` on the Cloudflare lane)
//! are installed via `Object.defineProperty(..., {configurable: false,
//! writable: false})` instead of a plain assignment, and — on the two lanes
//! where guest bundle code performs the install itself — as the FIRST
//! statement of that dispatch segment, before any guest handler body or a
//! microtask queued from one has a chance to run.
//!
//! These tests exercise the atomicity property directly: once installed, a
//! guest attempt to redirect the slot — whether a synchronous top-level
//! statement or one deferred via `queueMicrotask` — must fail (observably:
//! it throws, matching real bundles' ES-module strict-mode semantics), and
//! dispatch must still resolve to the ORIGINAL entrypoint. This is the same
//! off-graph capture mechanism `captured_dispatch.rs`'s identity-stability
//! test exercises for a plain reassignment; these tests additionally prove
//! the SLOT ITSELF resists redirection, on both the main context and a fresh
//! realm, across all three lanes that install a trusted entrypoint.
//!
//! `execute_script` evaluates as a classic (non-module) script, which is
//! sloppy-mode by default — assigning to a non-writable global property would
//! silently no-op there instead of throwing, masking the very distinction
//! this fix draws (module top-level code is always strict). Each guest-side
//! probe script below starts with `"use strict";` (node/Cloud Functions
//! lanes) or is itself an ES module (Cloudflare lane, which is always
//! strict) to match real bundles' actual evaluation mode.

use super::captured_dispatch::{create_bare_realm, with_captured_dispatch_test_runtime};
use super::*;
use crate::runtime::captured_dispatch::{call_captured_invocation, capture_invocation_targets};

/// Guest-side probe appended after an entrypoint install: attempts a direct
/// top-level reassignment, then a `queueMicrotask`-deferred one, recording
/// whether each attempt threw. Used by the node and Cloud Functions lanes,
/// whose probes run as separate `execute_script` classic-script calls
/// sharing one `globalThis` (rather than an ES module's lexical scope), so
/// state crosses call boundaries via `globalThis` flags.
fn reassignment_probe_script(global_name: &str) -> String {
    format!(
        r#""use strict";
let __probeDirectThrew = false;
try {{
  globalThis.{global_name} = function () {{ return {{ source: "IMPOSTOR_DIRECT" }}; }};
}} catch (error) {{
  __probeDirectThrew = true;
}}
globalThis.__nimbusProbeDirectThrew = __probeDirectThrew;

globalThis.__nimbusProbeMicrotaskSettled = false;
queueMicrotask(() => {{
  let microtaskThrew = false;
  try {{
    globalThis.{global_name} = function () {{ return {{ source: "IMPOSTOR_MICROTASK" }}; }};
  }} catch (error) {{
    microtaskThrew = true;
  }}
  globalThis.__nimbusProbeMicrotaskThrew = microtaskThrew;
  globalThis.__nimbusProbeMicrotaskSettled = true;
}});
undefined"#
    )
}

struct ReassignmentProbeResult {
    direct_threw: bool,
    microtask_settled: bool,
    microtask_threw: bool,
}

/// Run the reassignment probe against `realm` (or the main context when
/// `None`) and read back its flags. A second `execute_script` call forces a
/// fresh embedder/V8 boundary after the probe script's own top-level
/// completes, so the `Auto` microtasks policy has already drained the queued
/// callback by the time these reads happen.
/// This embedder does not auto-drain microtasks between separate
/// `execute_script` calls (confirmed by the explicit
/// `scope.perform_microtask_checkpoint()` calls elsewhere in the runtime,
/// e.g. `bootstrap/ops/test_runtime/ops_impl.rs`), so a `queueMicrotask`
/// callback queued by one `execute_script` call is NOT guaranteed to have run
/// by the time a later, separate `execute_script` call reads its effects.
/// Force the checkpoint explicitly rather than relying on incidental timing.
fn drain_microtasks(
    locked: &mut crate::backends::v8::embedder::JsRuntime,
    realm: Option<&crate::backends::v8::embedder::JsRealm>,
) {
    use crate::backends::v8::embedder::v8;
    let context = match realm {
        Some(realm) => realm.context().clone(),
        None => locked.main_context(),
    };
    let isolate = locked.v8_isolate();
    v8::scope_with_context!(let scope, isolate, &context);
    scope.perform_microtask_checkpoint();
}

fn run_reassignment_probe(
    locked: &mut crate::backends::v8::embedder::JsRuntime,
    realm: Option<&crate::backends::v8::embedder::JsRealm>,
    global_name: &str,
) -> ReassignmentProbeResult {
    let probe_script = reassignment_probe_script(global_name);
    match realm {
        Some(realm) => {
            realm
                .execute_script(
                    locked.v8_isolate(),
                    "guest_reassignment_probe.js",
                    probe_script,
                )
                .expect("reassignment probe should evaluate");
        }
        None => {
            locked
                .execute_script("guest_reassignment_probe.js", probe_script)
                .expect("reassignment probe should evaluate");
        }
    }
    drain_microtasks(locked, realm);

    let read = |locked: &mut crate::backends::v8::embedder::JsRuntime, expr: &str| {
        let value = match realm {
            Some(realm) => realm
                .execute_script(locked.v8_isolate(), "read_probe_flag.js", expr.to_string())
                .expect("reading probe flag should evaluate"),
            None => locked
                .execute_script("read_probe_flag.js", expr.to_string())
                .expect("reading probe flag should evaluate"),
        };
        deserialize_json_value(locked, value).expect("probe flag should deserialize")
    };

    let direct_threw = read(locked, "globalThis.__nimbusProbeDirectThrew");
    let microtask_settled = read(locked, "globalThis.__nimbusProbeMicrotaskSettled");
    let microtask_threw = read(locked, "globalThis.__nimbusProbeMicrotaskThrew");

    ReassignmentProbeResult {
        direct_threw: direct_threw
            .as_bool()
            .expect("direct_threw should be a bool"),
        microtask_settled: microtask_settled
            .as_bool()
            .expect("microtask_settled should be a bool"),
        microtask_threw: microtask_threw
            .as_bool()
            .expect("microtask_threw should be a bool"),
    }
}

fn assert_probe_survived(result: &ReassignmentProbeResult, global_name: &str) {
    assert!(
        result.direct_threw,
        "a direct top-level reassignment of globalThis.{global_name} must throw once the atomic \
         install has run"
    );
    assert!(
        result.microtask_settled,
        "the queueMicrotask-deferred reassignment attempt against globalThis.{global_name} must \
         have run by the time this probe reads its result"
    );
    assert!(
        result.microtask_threw,
        "a queueMicrotask-deferred reassignment of globalThis.{global_name} must also throw — \
         the atomic install closes the window for BOTH a direct and a deferred redirect attempt"
    );
}

// --- Node/Convex lane: __nimbusInvoke installed the way
// packages/codegen/src/emit/runtime_bundle_dispatch_global_invoke.mjs emits
// it (defineProperty, as the first statement of the dispatch segment). ---

const NODE_LANE_INSTALL: &str = r#"Object.defineProperty(globalThis, "__nimbusInvoke", {
  value: function () { return { source: "REAL_NODE_LANE" }; },
  configurable: false,
  enumerable: false,
  writable: false,
});
undefined"#;

pub(super) const CAPTURE_ORDERING_NODE_LANE_MAIN_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-capture-ordering-node-lane-main-context",
        "cooperative-startup-snapshot",
        "Band B-FIX CAPTURE-ORDERING (1): on the main context, a guest reassignment of \
         globalThis.__nimbusInvoke — direct or queueMicrotask-deferred — throws after the \
         node/Convex lane's atomic install, and captured dispatch still runs the original \
         entrypoint",
        "runtime::tests::capture_ordering::node_lane_main_context_reassignment_fails_subprocess",
    );

#[test]
fn node_lane_main_context_reassignment_fails() {
    run_v8_sensitive_runtime_test_in_subprocess(CAPTURE_ORDERING_NODE_LANE_MAIN_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn node_lane_main_context_reassignment_fails_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        locked
            .execute_script("install_node_lane_invoke.js", NODE_LANE_INSTALL)
            .expect("installing the node-lane entrypoint should succeed");

        let probe = run_reassignment_probe(locked, None, "__nimbusInvoke");
        assert_probe_survived(&probe, "__nimbusInvoke");

        capture_invocation_targets(locked, None, crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed despite the reassignment attempts");
        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let result = {
            let value = call_captured_invocation(
                locked,
                None,
                &request_json,
                crate::RuntimeGuestSemantics::Host,
                None,
            )
            .expect("captured dispatch should still run the original entrypoint");
            deserialize_json_value(locked, value).expect("result should deserialize")
        };
        assert_eq!(
            result["source"], "REAL_NODE_LANE",
            "captured dispatch must resolve to the original entrypoint, not either impostor: \
             {result}"
        );
    });
}

pub(super) const CAPTURE_ORDERING_NODE_LANE_FRESH_REALM_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-capture-ordering-node-lane-fresh-realm",
        "cooperative-startup-snapshot",
        "Band B-FIX CAPTURE-ORDERING (1): the same guarantee holds on a fresh realm, not just \
         the main context",
        "runtime::tests::capture_ordering::node_lane_fresh_realm_reassignment_fails_subprocess",
    );

#[test]
fn node_lane_fresh_realm_reassignment_fails() {
    run_v8_sensitive_runtime_test_in_subprocess(CAPTURE_ORDERING_NODE_LANE_FRESH_REALM_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn node_lane_fresh_realm_reassignment_fails_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        let realm = create_bare_realm(locked);

        realm
            .execute_script(
                locked.v8_isolate(),
                "install_node_lane_invoke.js",
                NODE_LANE_INSTALL,
            )
            .expect("installing the node-lane entrypoint should succeed");

        let probe = run_reassignment_probe(locked, Some(&realm), "__nimbusInvoke");
        assert_probe_survived(&probe, "__nimbusInvoke");

        capture_invocation_targets(locked, Some(&realm), crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed despite the reassignment attempts");
        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let result = {
            let value = call_captured_invocation(
                locked,
                Some(&realm),
                &request_json,
                crate::RuntimeGuestSemantics::Host,
                None,
            )
            .expect("captured dispatch should still run the original entrypoint");
            deserialize_json_value(locked, value).expect("result should deserialize")
        };
        assert_eq!(
            result["source"], "REAL_NODE_LANE",
            "captured dispatch must resolve to the original entrypoint on a fresh realm too: \
             {result}"
        );

        crate::runtime::realm_lifecycle::destroy_fresh_realm(locked, realm);
    });
}

// --- Cloud Functions lane: __nimbusInvoke installed the way
// packages/codegen/src/cloud_functions/runtime_sources.mjs emits it — same
// defineProperty mechanism, reached from behind eager static imports and a
// createInvocationDispatcher(...) call instead of the node lane's inline
// closure, but the atomicity property under test is identical. ---

const CLOUD_FUNCTIONS_LANE_INSTALL: &str = r#"const __nimbusCollectedTargets = [];
function createInvocationDispatcher() {
  return function () { return { source: "REAL_CLOUD_FUNCTIONS_LANE" }; };
}
Object.defineProperty(globalThis, "__nimbusInvoke", {
  value: createInvocationDispatcher(__nimbusCollectedTargets),
  configurable: false,
  enumerable: false,
  writable: false,
});
undefined"#;

pub(super) const CAPTURE_ORDERING_CLOUD_FUNCTIONS_LANE_MAIN_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-capture-ordering-cloud-functions-lane-main-context",
        "cooperative-startup-snapshot",
        "Band B-FIX CAPTURE-ORDERING (1): on the main context, a guest reassignment of \
         globalThis.__nimbusInvoke — direct or queueMicrotask-deferred — throws after the \
         Cloud Functions lane's atomic install, and captured dispatch still runs the original \
         entrypoint",
        "runtime::tests::capture_ordering::cloud_functions_lane_main_context_reassignment_fails_subprocess",
    );

#[test]
fn cloud_functions_lane_main_context_reassignment_fails() {
    run_v8_sensitive_runtime_test_in_subprocess(CAPTURE_ORDERING_CLOUD_FUNCTIONS_LANE_MAIN_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn cloud_functions_lane_main_context_reassignment_fails_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        locked
            .execute_script(
                "install_cloud_functions_lane_invoke.js",
                CLOUD_FUNCTIONS_LANE_INSTALL,
            )
            .expect("installing the Cloud Functions-lane entrypoint should succeed");

        let probe = run_reassignment_probe(locked, None, "__nimbusInvoke");
        assert_probe_survived(&probe, "__nimbusInvoke");

        capture_invocation_targets(locked, None, crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed despite the reassignment attempts");
        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let result = {
            let value = call_captured_invocation(
                locked,
                None,
                &request_json,
                crate::RuntimeGuestSemantics::Host,
                None,
            )
            .expect("captured dispatch should still run the original entrypoint");
            deserialize_json_value(locked, value).expect("result should deserialize")
        };
        assert_eq!(
            result["source"], "REAL_CLOUD_FUNCTIONS_LANE",
            "captured dispatch must resolve to the original entrypoint, not either impostor: \
             {result}"
        );
    });
}

pub(super) const CAPTURE_ORDERING_CLOUD_FUNCTIONS_LANE_FRESH_REALM_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-capture-ordering-cloud-functions-lane-fresh-realm",
        "cooperative-startup-snapshot",
        "Band B-FIX CAPTURE-ORDERING (1): the same guarantee holds on a fresh realm, not just \
         the main context",
        "runtime::tests::capture_ordering::cloud_functions_lane_fresh_realm_reassignment_fails_subprocess",
    );

#[test]
fn cloud_functions_lane_fresh_realm_reassignment_fails() {
    run_v8_sensitive_runtime_test_in_subprocess(
        CAPTURE_ORDERING_CLOUD_FUNCTIONS_LANE_FRESH_REALM_CASE,
    );
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn cloud_functions_lane_fresh_realm_reassignment_fails_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        let realm = create_bare_realm(locked);

        realm
            .execute_script(
                locked.v8_isolate(),
                "install_cloud_functions_lane_invoke.js",
                CLOUD_FUNCTIONS_LANE_INSTALL,
            )
            .expect("installing the Cloud Functions-lane entrypoint should succeed");

        let probe = run_reassignment_probe(locked, Some(&realm), "__nimbusInvoke");
        assert_probe_survived(&probe, "__nimbusInvoke");

        capture_invocation_targets(locked, Some(&realm), crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed despite the reassignment attempts");
        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let result = {
            let value = call_captured_invocation(
                locked,
                Some(&realm),
                &request_json,
                crate::RuntimeGuestSemantics::Host,
                None,
            )
            .expect("captured dispatch should still run the original entrypoint");
            deserialize_json_value(locked, value).expect("result should deserialize")
        };
        assert_eq!(
            result["source"], "REAL_CLOUD_FUNCTIONS_LANE",
            "captured dispatch must resolve to the original entrypoint on a fresh realm too: \
             {result}"
        );

        crate::runtime::realm_lifecycle::destroy_fresh_realm(locked, realm);
    });
}

// --- Cloudflare lane: __nimbusInvokeCloudflareWorkerFetch is installed by
// the host-authored cloudflare_workers_runtime.js BOOTSTRAP script, always
// run before any guest worker module loads — so unlike the two lanes above,
// there is no guest-controlled install step to race. The property under
// test here is that the guest WORKER MODULE (an ES module, sharing this
// realm's globalThis) cannot claw the already-locked slot back once its own
// top-level code (or a microtask queued from it) runs. These tests drive
// the real end-to-end `invoke_bundle_for_tenant` path — real bootstrap, real
// module loading, real `capture_invocation_targets`/`call_captured_invocation`
// — rather than hand-rolling a fake module namespace, so a passing result
// proves the production dispatch path, not just the low-level capture API in
// isolation. Module top-level code is always strict, so a plain reassignment
// attempt here throws without needing an explicit "use strict" prologue. ---

fn cloudflare_reassignment_probe_worker_source() -> &'static str {
    r#"
let directThrew = false;
try {
  globalThis.__nimbusInvokeCloudflareWorkerFetch = function () {
    return { source: "IMPOSTOR_DIRECT" };
  };
} catch (error) {
  directThrew = true;
}

let microtaskThrew = null;
let microtaskSettled = false;
queueMicrotask(() => {
  try {
    globalThis.__nimbusInvokeCloudflareWorkerFetch = function () {
      return { source: "IMPOSTOR_MICROTASK" };
    };
    microtaskThrew = false;
  } catch (error) {
    microtaskThrew = true;
  }
  microtaskSettled = true;
});

export default {
  async fetch(request, env, ctx) {
    // Force a microtask-queue drain so the queueMicrotask callback above
    // (queued during this module's own top-level evaluation) has definitely
    // run before the flags it sets are read.
    await Promise.resolve();
    return new Response(JSON.stringify({ directThrew, microtaskThrew, microtaskSettled }));
  },
};
"#
}

fn cloudflare_worker_fetch_request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::CloudflareWorkerFetch,
        function_name: "worker:fetch".to_string(),
        args: serde_json::json!({
            "request": {
                "url": "https://example.com/",
                "method": "GET",
            },
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn assert_cloudflare_probe_survived(body: &Value) {
    assert_eq!(
        body["directThrew"],
        serde_json::json!(true),
        "a direct top-level reassignment of globalThis.__nimbusInvokeCloudflareWorkerFetch from \
         the guest worker module must throw — the bootstrap-installed slot is already locked \
         before the module ever loads: {body}"
    );
    assert_eq!(
        body["microtaskSettled"],
        serde_json::json!(true),
        "the queueMicrotask-deferred reassignment attempt must have run by the time the worker's \
         fetch handler reads its result: {body}"
    );
    assert_eq!(
        body["microtaskThrew"],
        serde_json::json!(true),
        "a queueMicrotask-deferred reassignment of globalThis.__nimbusInvokeCloudflareWorkerFetch \
         must also throw: {body}"
    );
}

#[tokio::test]
async fn cloudflare_lane_main_context_reassignment_fails() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(&bundle_path, cloudflare_reassignment_probe_worker_source())
        .expect("worker bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_fetch_request(),
            "tenant-a",
        )
        .await
        .expect(
            "the worker's fetch handler must still run through the real, unreplaced trampoline",
        );

    let body: Value = serde_json::from_str(
        result["body"]
            .as_str()
            .expect("serialized response body should be text"),
    )
    .expect("response body should be JSON");
    assert_cloudflare_probe_survived(&body);
}

// This test also doubles as the RED/GREEN oracle for Band B-FIX CLOUDFLARE
// REALM ISOLATION (finding 2): `call_captured_invocation`'s dynamic
// `import(specifier)` must evaluate via `realm.execute_script` (scoped to
// THIS fresh/recycled realm) rather than `runtime.execute_script` (always
// the main realm) once a realm is present. Reverting that branch to
// unconditionally use `runtime.execute_script` was confirmed to break this
// exact test — the module namespace import resolves against the wrong
// realm's registry, so the worker's fetch handler promise never settles
// ("Promise resolution is still pending but the event loop has already
// resolved") — while `cloudflare_lane_main_context_reassignment_fails`
// above (no realm, so no realm/main distinction) correctly keeps passing as
// a non-regression control.
#[tokio::test]
async fn cloudflare_lane_fresh_realm_reassignment_fails() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("worker.mjs");
    std::fs::write(&bundle_path, cloudflare_reassignment_probe_worker_source())
        .expect("worker bundle should write");

    // WarmContextRecycle: each invocation runs in a recycled realm rather
    // than the runtime's main realm, exercising driver/loading.rs's
    // fresh-realm path (capture + dispatch against `Some(realm)`) instead of
    // the main-context path the test above exercises.
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(crate::limits::RuntimePolicy::new(
            cooperative_context_recycle_runtime_test_limits(),
        )),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &cloudflare_worker_fetch_request(),
            "tenant-a",
        )
        .await
        .expect(
            "the worker's fetch handler must still run through the real, unreplaced trampoline \
             on a fresh realm too",
        );

    let body: Value = serde_json::from_str(
        result["body"]
            .as_str()
            .expect("serialized response body should be text"),
    )
    .expect("response body should be JSON");
    assert_cloudflare_probe_survived(&body);
}
