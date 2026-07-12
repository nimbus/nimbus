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
use crate::runtime::captured_dispatch::{call_captured_invocation, capture_invocation_targets};
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

    // Host capture at "bundle load", before any guest handler body runs.
    capture_invocation_targets(&mut locked, None).expect("capture should succeed");

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
}
