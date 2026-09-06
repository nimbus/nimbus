//! HG0/HG5 identity-stability: the host dispatches invocations through the
//! entrypoint captured off the guest-reachable graph at bundle load, never
//! `globalThis.__nimbusInvoke` re-read by name at call time. This is the
//! flagship fix — it removes the warm-pool cross-invocation amplifier where a
//! guest handler that reassigned/deleted the global in one invocation could be
//! handed a later same-tenant invocation's request/args/auth on the same warm
//! isolate.
//!
//! These run in a subprocess (like the other V8-state-sensitive runtime tests)
//! to isolate isolate construction from the shared read-only heap cage.

use super::*;
use crate::backends::v8::V8WorkerRuntimePool;
use crate::backends::v8::embedder::scope;
use crate::runtime::captured_dispatch::{
    call_captured_invocation, capture_invocation_targets, captured_invoke_is,
};
use crate::test_support::acquire_runtime_suite_lock_blocking;

pub(super) const CAPTURED_INVOKE_IDENTITY_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-invoke-identity-stability",
        "cooperative-startup-snapshot",
        "host dispatch uses the invoke entrypoint captured at load, not globalThis.__nimbusInvoke \
         re-read by name, so guest reassignment or deletion cannot redirect it",
        "runtime::tests::captured_dispatch::captured_invoke_survives_guest_reassignment_and_delete_subprocess",
    );

#[test]
fn captured_invoke_survives_guest_reassignment_and_delete() {
    run_v8_sensitive_runtime_test_in_subprocess(CAPTURED_INVOKE_IDENTITY_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn captured_invoke_survives_guest_reassignment_and_delete_subprocess() {
    let _guard = acquire_runtime_suite_lock_blocking();
    // Deno schedules delayed V8 foreground tasks through Tokio during isolate
    // construction; keep a current-thread runtime entered for the isolate's
    // whole lifetime (mirrors the locker tests).
    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("captured-dispatch test tokio runtime should build");
    let _tokio_enter = tokio_runtime.enter();

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let runtime_instance = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        cooperative_startup_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime_with_options(&runtime_instance, &bundle, true)
        .expect("captured-dispatch runtime should build from snapshot")
        .runtime;

    let mut locked = runtime.acquire_v8_lock();

    // The trusted dispatcher a well-behaved bundle installs at the end of its
    // module evaluation. The closure captures a token no impostor can produce,
    // so a result carrying it proves the SAME function object executed (identity
    // stability), not merely equivalent behavior.
    locked
        .execute_script(
            "install_real_invoke.js",
            r#"globalThis.__nimbusInvoke = function (request) {
  return { source: "REAL", token: 0x5eaf00d, fn: request.function_name };
};
undefined"#,
        )
        .expect("installing the real invoke entrypoint should succeed");

    // Retain the exact function object installed above, so capture can be
    // checked for V8 strict/reference equality (`===`), not just behavior
    // (Band B-FIX IDENTITY TEST WEAK).
    let real_invoke = {
        let value = locked
            .execute_script("read_real_invoke.js", "globalThis.__nimbusInvoke")
            .expect("reading back the real invoke entrypoint should succeed");
        scope!(scope, &mut locked);
        let local = v8::Local::new(scope, value);
        let function = v8::Local::<v8::Function>::try_from(local)
            .expect("globalThis.__nimbusInvoke must be a function");
        v8::Global::new(scope, function)
    };

    // Host capture at "bundle load", before any guest handler body runs.
    capture_invocation_targets(&mut locked, crate::RuntimeGuestSemantics::Host)
        .expect("capture should succeed");

    // The private slot must hold the SAME function object read back above —
    // reference identity, not merely a value that behaves the same way an
    // impostor could also reproduce.
    assert!(
        captured_invoke_is(&mut locked, &real_invoke).expect("identity check should evaluate"),
        "captured __nimbusInvoke must be the exact function object installed by the bundle \
         (V8 strict reference equality), not merely a behaviorally-equivalent value"
    );

    // A guest handler in an earlier invocation reassigns the global to an
    // impostor. On a warm isolate this reassignment persists into the next
    // invocation's trusted path.
    locked
        .execute_script(
            "guest_reassigns_invoke.js",
            r#"globalThis.__nimbusInvoke = function () {
  return { source: "IMPOSTOR", token: 0 };
};
undefined"#,
        )
        .expect("guest reassignment should evaluate");

    // Reading globalThis.__nimbusInvoke by NAME — what the host used to do on
    // every invocation — now hits the impostor. This is the vulnerability the
    // fix closes; asserting it keeps the test honest (red path is live).
    let name_based = {
        let value = locked
            .execute_script(
                "name_based_dispatch.js",
                r#"globalThis.__nimbusInvoke({ function_name: "messages:list", kind: "action" })"#,
            )
            .expect("name-based dispatch should evaluate");
        deserialize_json_value(&mut locked, value).expect("name-based result should deserialize")
    };
    assert_eq!(
        name_based["source"], "IMPOSTOR",
        "re-reading globalThis.__nimbusInvoke by name must observe the guest impostor (the closed vulnerability): {name_based}"
    );

    // The guest also deletes the global outright.
    locked
        .execute_script(
            "guest_deletes_invoke.js",
            "delete globalThis.__nimbusInvoke; undefined",
        )
        .expect("guest deletion should evaluate");

    // The host dispatch path reads the CAPTURED reference, so it invokes the
    // real entrypoint even though the global was reassigned and then deleted.
    let request_json =
        r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
    let captured = {
        let value = call_captured_invocation(
            &mut locked,
            &request_json,
            crate::RuntimeGuestSemantics::Host,
            None,
        )
        .expect("captured dispatch should return a value even after tampering");
        deserialize_json_value(&mut locked, value).expect("captured result should deserialize")
    };
    assert_eq!(
        captured["source"], "REAL",
        "captured dispatch must run the real entrypoint after guest reassignment+delete: {captured}"
    );
    assert_eq!(
        captured["token"],
        serde_json::json!(0x5eaf00d),
        "captured dispatch must run the SAME function object captured at load (identity stability): {captured}"
    );
    assert_eq!(
        captured["fn"], "messages:list",
        "captured dispatch must forward the real request to the real entrypoint: {captured}"
    );

    // Identity check again, after the guest reassignment/delete/dispatch
    // sequence: the private slot itself was never touched by any of it.
    assert!(
        captured_invoke_is(&mut locked, &real_invoke)
            .expect("post-dispatch identity check should evaluate"),
        "captured __nimbusInvoke must still be the exact original function object after guest \
         reassignment, deletion, and dispatch"
    );
}

/// Shared scaffolding for the Band B-FIX findings below: build a fresh V8
/// runtime from the cooperative-startup-snapshot policy and run `body` with
/// the acquired lock. Every caller needs the same tokio-entered, pool-backed
/// runtime the identity test above builds; factoring it out keeps each
/// finding's test focused on its own bundle source and assertions.
pub(super) fn with_captured_dispatch_test_runtime(
    body: impl FnOnce(&mut crate::backends::v8::embedder::JsRuntime),
) {
    let _guard = acquire_runtime_suite_lock_blocking();
    let tokio_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("captured-dispatch test tokio runtime should build");
    let _tokio_enter = tokio_runtime.enter();

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    let runtime_instance = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        cooperative_startup_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime_with_options(&runtime_instance, &bundle, true)
        .expect("captured-dispatch runtime should build from snapshot")
        .runtime;

    let mut locked = runtime.acquire_v8_lock();
    body(&mut locked);
}

pub(super) const NON_FUNCTION_ENTRYPOINT_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-dispatch-non-function-entrypoint",
        "cooperative-startup-snapshot",
        "Band B-FIX HG5 BEGIN-HOOK FAIL-OPEN (3b): capture hard-fails when a trusted entrypoint name \
     is present but is not a callable function, instead of treating it as lane-absence",
        "runtime::tests::captured_dispatch::capture_rejects_non_function_entrypoint_subprocess",
    );

#[test]
fn capture_rejects_non_function_entrypoint() {
    run_v8_sensitive_runtime_test_in_subprocess(NON_FUNCTION_ENTRYPOINT_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn capture_rejects_non_function_entrypoint_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        // A bundle (or a guest clobbering the global before host capture runs
        // on a warm isolate) that installs a non-function value under the
        // trusted entrypoint name.
        locked
            .execute_script(
                "install_non_function_invoke.js",
                "globalThis.__nimbusInvoke = 42; undefined",
            )
            .expect("installing the non-function value should succeed");

        // Host guest-semantics: the ConvexDefault begin-hook requirement does
        // not apply, isolating this assertion to the non-function check.
        let error = capture_invocation_targets(locked, crate::RuntimeGuestSemantics::Host)
            .expect_err(
                "capture must hard-fail when globalThis.__nimbusInvoke is present but not a \
                 function",
            );
        let message = error.to_string();
        assert!(
            message.contains("__nimbusInvoke")
                && message.contains("is present but is not a function"),
            "capture failure must identify the offending global and why it was rejected: {message}"
        );
    });
}

pub(super) const HEAP_LIMIT_DURING_REQUEST_ALLOCATION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-dispatch-heap-limit-during-request-allocation",
        "cooperative-startup-snapshot",
        "Band B-FIX ERROR CLASSIFICATION (5a): an execution termination during request-string/JSON \
         allocation, before the entrypoint call, still normalizes to the exact \"execution \
         terminated\" string classify.rs keys HeapLimitExceeded on",
        "runtime::tests::captured_dispatch::execution_termination_during_request_allocation_classifies_as_terminated_subprocess",
    );

#[test]
fn execution_termination_during_request_allocation_classifies_as_terminated() {
    run_v8_sensitive_runtime_test_in_subprocess(HEAP_LIMIT_DURING_REQUEST_ALLOCATION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn execution_termination_during_request_allocation_classifies_as_terminated_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        locked
            .execute_script(
                "install_real_invoke.js",
                r#"globalThis.__nimbusInvoke = function () { return { ok: true }; };
undefined"#,
            )
            .expect("installing invoke should succeed");
        capture_invocation_targets(locked, crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed");

        // Simulate a watchdog/heap-limit trip that fires before dispatch ever
        // reaches the entrypoint call — i.e. during the request-string
        // allocation or JSON.parse the TryCatch now covers (Band B-FIX ERROR
        // CLASSIFICATION). `terminate_execution` sets the isolate's
        // termination flag; every subsequent V8 allocation returns an empty
        // MaybeLocal until it is canceled.
        let isolate_handle = locked.v8_isolate().thread_safe_handle();
        isolate_handle.terminate_execution();

        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let error = call_captured_invocation(
            locked,
            &request_json,
            crate::RuntimeGuestSemantics::Host,
            None,
        )
        .expect_err("a termination during request allocation must surface as an error");

        // Reset the termination flag so the isolate can still be torn down
        // cleanly at the end of the test.
        locked.v8_isolate().cancel_terminate_execution();

        match error {
            NimbusRuntimeError::JavaScript(message) => assert_eq!(
                message, "execution terminated",
                "a termination during request allocation must produce the EXACT string \
                 classify.rs keys HeapLimitExceeded classification on, not a bespoke Contract \
                 message"
            ),
            other => panic!(
                "expected NimbusRuntimeError::JavaScript(\"execution terminated\"), got {other:?}"
            ),
        }
    });
}

pub(super) const ORDINARY_EXCEPTION_PRESERVES_STACK_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-dispatch-ordinary-exception-preserves-stack",
        "cooperative-startup-snapshot",
        "Band B-FIX ERROR CLASSIFICATION (5b): an ordinary guest exception is formatted with \
         deno_core's stack-aware JsError, not a lossy to_rust_string_lossy conversion that drops \
         frame/location information",
        "runtime::tests::captured_dispatch::ordinary_exception_preserves_stack_frame_subprocess",
    );

#[test]
fn ordinary_exception_preserves_stack_frame() {
    run_v8_sensitive_runtime_test_in_subprocess(ORDINARY_EXCEPTION_PRESERVES_STACK_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn ordinary_exception_preserves_stack_frame_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        // Named so the stack frame the fix must preserve carries a
        // distinguishing marker a lossy `Error#toString()` conversion (the
        // pre-fix behavior) would never include.
        locked
            .execute_script(
                "install_exploding_invoke.js",
                r#"globalThis.__nimbusInvoke = function explodingInvoke() {
  throw new Error("boom from finding 5b");
};
undefined"#,
            )
            .expect("installing the exploding invoke entrypoint should succeed");
        capture_invocation_targets(locked, crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed");

        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let error = call_captured_invocation(
            locked,
            &request_json,
            crate::RuntimeGuestSemantics::Host,
            None,
        )
        .expect_err("the guest exception must propagate as a dispatch error");
        let message = error.to_string();
        assert!(
            message.contains("boom from finding 5b"),
            "the error message must still carry the exception's own message: {message}"
        );
        assert!(
            message.contains("explodingInvoke"),
            "the error message must carry stack-frame/location information (the throwing \
             function's name), proving it went through deno_core's JsError formatting rather \
             than a lossy plain-string exception conversion: {message}"
        );
    });
}
