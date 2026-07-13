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

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        cooperative_startup_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
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
    capture_invocation_targets(&mut locked, None, crate::RuntimeGuestSemantics::Host)
        .expect("capture should succeed");

    // The private slot must hold the SAME function object read back above —
    // reference identity, not merely a value that behaves the same way an
    // impostor could also reproduce.
    assert!(
        captured_invoke_is(&mut locked, None, &real_invoke)
            .expect("identity check should evaluate"),
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
            None,
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
        captured_invoke_is(&mut locked, None, &real_invoke)
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

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        cooperative_startup_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("captured-dispatch runtime should build from snapshot")
        .runtime;

    let mut locked = runtime.acquire_v8_lock();
    body(&mut locked);
}

/// A raw V8 realm with NONE of `install_bootstrap_in_realm`'s scripts run —
/// in particular, none of `nimbus_guest_semantics.js`'s
/// `Object.defineProperty(globalThis, "__nimbusBeginGuestInvocation", ...)`.
/// The real bootstrap installs that hook as a non-configurable,
/// non-writable, always-present no-op on every lane (including Host), so it
/// can never be made absent or overridden on a normally-bootstrapped realm —
/// exactly the guarantee the fix relies on in production. Findings 3a and 4
/// need a realm where that guarantee has NOT (yet) been established, to
/// exercise what `capture_invocation_targets`/`call_captured_invocation` do
/// when it genuinely has not run — e.g. a future build/profile regression
/// that drops the guest-semantics bootstrap script.
pub(super) fn create_bare_realm(
    runtime: &mut crate::backends::v8::embedder::JsRuntime,
) -> crate::backends::v8::embedder::JsRealm {
    runtime
        .create_realm(crate::backends::v8::embedder::CreateRealmOptions {
            module_loader: None,
        })
        .expect("bare realm should create")
}

pub(super) const CONVEX_DEFAULT_MISSING_BEGIN_HOOK_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-dispatch-convex-default-missing-begin-hook",
        "cooperative-startup-snapshot",
        "Band B-FIX HG5 BEGIN-HOOK FAIL-OPEN (3a): capture hard-fails a ConvexDefault realm that \
         never had __nimbusBeginGuestInvocation installed, instead of silently loading without a \
         determinism-reset hook",
        "runtime::tests::captured_dispatch::convex_default_capture_requires_begin_guest_invocation_hook_subprocess",
    );

#[test]
fn convex_default_capture_requires_begin_guest_invocation_hook() {
    run_v8_sensitive_runtime_test_in_subprocess(CONVEX_DEFAULT_MISSING_BEGIN_HOOK_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn convex_default_capture_requires_begin_guest_invocation_hook_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        let realm = create_bare_realm(locked);

        // Sanity: on a realm where the guest-semantics bootstrap never ran,
        // the hook is genuinely absent, not merely reassigned.
        let hook_is_undefined = {
            let value = realm
                .execute_script(
                    locked.v8_isolate(),
                    "check_begin_hook_absent.js",
                    "typeof globalThis.__nimbusBeginGuestInvocation === \"undefined\"",
                )
                .expect("checking the begin hook's presence should evaluate");
            deserialize_json_value(locked, value).expect("presence check should deserialize")
        };
        assert_eq!(
            hook_is_undefined,
            serde_json::json!(true),
            "test setup invariant: a bare, un-bootstrapped realm must not have \
             __nimbusBeginGuestInvocation installed"
        );

        // Install the invoke entrypoint but never the begin hook.
        realm
            .execute_script(
                locked.v8_isolate(),
                "install_invoke_only.js",
                r#"globalThis.__nimbusInvoke = function () { return { ok: true }; };
undefined"#,
            )
            .expect("installing invoke should succeed");

        let error = capture_invocation_targets(
            locked,
            Some(&realm),
            crate::RuntimeGuestSemantics::ConvexDefault,
        )
        .expect_err(
            "capture must hard-fail a ConvexDefault realm that never had \
             __nimbusBeginGuestInvocation installed",
        );
        let message = error.to_string();
        assert!(
            message.contains("__nimbusBeginGuestInvocation") && message.contains("ConvexDefault"),
            "capture failure must name the missing hook and the lane that requires it: {message}"
        );

        crate::runtime::realm_lifecycle::destroy_fresh_realm(locked, realm);
    });
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
        let error = capture_invocation_targets(locked, None, crate::RuntimeGuestSemantics::Host)
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

pub(super) const DETERMINISM_HOOK_SWALLOWED_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-captured-dispatch-determinism-hook-swallowed",
        "cooperative-startup-snapshot",
        "Band B-FIX DETERMINISM-HOOK SWALLOWED (4): a faulting __nimbusBeginGuestInvocation aborts \
         the invocation before __nimbusInvoke ever runs, instead of proceeding with a stale \
         clock/PRNG",
        "runtime::tests::captured_dispatch::determinism_hook_fault_aborts_before_invoke_runs_subprocess",
    );

#[test]
fn determinism_hook_fault_aborts_before_invoke_runs() {
    run_v8_sensitive_runtime_test_in_subprocess(DETERMINISM_HOOK_SWALLOWED_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate V8 isolate state"]
fn determinism_hook_fault_aborts_before_invoke_runs_subprocess() {
    with_captured_dispatch_test_runtime(|locked| {
        // On a normally-bootstrapped realm, __nimbusBeginGuestInvocation is
        // installed non-configurable/non-writable by nimbus_guest_semantics.js
        // before any bundle code runs — a plain `globalThis.X = ...`
        // reassignment there silently no-ops (non-strict-mode semantics: no
        // throw, no change) rather than installing a faulting stub. A bare
        // realm never had that bootstrap script run, so the FIRST definition
        // of the name here genuinely takes effect, letting this test install
        // a hook that actually throws.
        let realm = create_bare_realm(locked);

        realm
            .execute_script(
                locked.v8_isolate(),
                "install_convex_default_bundle.js",
                r#"globalThis.__nimbusInvokeCalled = false;
globalThis.__nimbusBeginGuestInvocation = function () {
  throw new Error("determinism hook boom");
};
globalThis.__nimbusInvoke = function () {
  globalThis.__nimbusInvokeCalled = true;
  return { ok: true };
};
undefined"#,
            )
            .expect("installing the ConvexDefault bundle should succeed");

        capture_invocation_targets(
            locked,
            Some(&realm),
            crate::RuntimeGuestSemantics::ConvexDefault,
        )
        .expect("capture should succeed: both hooks are present and callable");

        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let error = call_captured_invocation(
            locked,
            Some(&realm),
            &request_json,
            crate::RuntimeGuestSemantics::ConvexDefault,
            None,
        )
        .expect_err("a faulting determinism hook must abort dispatch, not run invoke anyway");
        let message = error.to_string();
        assert!(
            message.contains("determinism hook boom"),
            "the determinism-hook exception must propagate as the dispatch error: {message}"
        );

        let invoke_called = {
            let value = realm
                .execute_script(
                    locked.v8_isolate(),
                    "read_invoke_called.js",
                    "globalThis.__nimbusInvokeCalled",
                )
                .expect("reading back the invoke-called flag should succeed");
            deserialize_json_value(locked, value).expect("invoke-called flag should deserialize")
        };
        assert_eq!(
            invoke_called,
            serde_json::json!(false),
            "__nimbusInvoke must never run when __nimbusBeginGuestInvocation faulted first"
        );

        crate::runtime::realm_lifecycle::destroy_fresh_realm(locked, realm);
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
        capture_invocation_targets(locked, None, crate::RuntimeGuestSemantics::Host)
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
            None,
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
        capture_invocation_targets(locked, None, crate::RuntimeGuestSemantics::Host)
            .expect("capture should succeed");

        let request_json =
            r#"{"kind":"action","function_name":"messages:list","args":null}"#.to_string();
        let error = call_captured_invocation(
            locked,
            None,
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
