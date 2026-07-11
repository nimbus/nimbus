//! Convex default-runtime guest semantics (RuntimeGuestSemantics::ConvexDefault):
//! seeded deterministic Math.random, frozen invocation clock, deploy-pinned
//! performance.timeOrigin, fetch-in-actions-only, the documented Node-API
//! subset (process.env, node:async_hooks), and the WebAssembly API.

use super::support::*;
use super::*;

fn convex_semantics_limits() -> RuntimeLimits {
    RuntimeLimits {
        guest_semantics: crate::RuntimeGuestSemantics::ConvexDefault,
        ..run_to_completion_snapshot_runtime_test_limits()
    }
}

fn convex_semantics_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(convex_semantics_limits()))
}

fn query_request(function_name: &str) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn request_of_kind(kind: InvocationKind, function_name: &str) -> InvocationRequest {
    InvocationRequest {
        kind,
        ..query_request(function_name)
    }
}

async fn invoke_convex_semantics_bundle(bundle_source: &str, request: &InvocationRequest) -> Value {
    let (_tempdir, bundle_path) = write_app_style_bundle(bundle_source);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        convex_semantics_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    runtime
        .invoke_bundle_for_tenant(&RuntimeBundle::new(&bundle_path), request, "tenant-a")
        .await
        .expect("convex-semantics bundle invocation should succeed")
}

const IMPORT_STABILITY_BUNDLE: &str = r#"
const importRandom = Math.random();
const importNow = Date.now();

globalThis.__nimbusInvoke = async function () {
  return {
    importRandom,
    importNow,
    localFirst: Math.random(),
    localSecond: Math.random(),
    timeOrigin: performance.timeOrigin,
  };
};

export {};
"#;

#[tokio::test]
async fn convex_semantics_math_random_seeded_and_import_values_stable_across_runs() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(IMPORT_STABILITY_BUNDLE);
    let request = query_request("messages:list");

    let mut runs = Vec::new();
    for _ in 0..2 {
        // A fresh runtime per run: the module graph re-evaluates, so
        // module-scope values are only equal if the import phase is really
        // deploy-seeded rather than wall-clock/entropy-backed.
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            convex_semantics_policy(),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let result = runtime
            .invoke_bundle_for_tenant(&RuntimeBundle::new(&bundle_path), &request, "tenant-a")
            .await
            .expect("import-stability invocation should succeed");
        runs.push(result);
    }

    let first = &runs[0];
    let second = &runs[1];

    // Module-scope values are deploy-frozen: identical across runs.
    assert_eq!(
        first["importRandom"], second["importRandom"],
        "module-scope Math.random() must be deploy-seeded and stable across runs"
    );
    assert_eq!(
        first["importNow"], second["importNow"],
        "module-scope Date.now() must be the deploy timestamp on every run"
    );
    assert_eq!(
        first["timeOrigin"], second["timeOrigin"],
        "performance.timeOrigin must be deploy-pinned and stable across runs"
    );
    assert_eq!(
        first["timeOrigin"], first["importNow"],
        "performance.timeOrigin must equal the deploy timestamp module code observed"
    );

    // Within one run the seeded stream still advances.
    assert_ne!(
        first["localFirst"], first["localSecond"],
        "two Math.random() calls within one invocation must differ"
    );
    // Across runs the per-invocation seed is fresh entropy.
    assert_ne!(
        first["localFirst"], second["localFirst"],
        "per-invocation Math.random() streams must be re-seeded per run"
    );
    let import_random = first["importRandom"]
        .as_f64()
        .expect("importRandom should be a number");
    assert!(
        (0.0..1.0).contains(&import_random),
        "seeded Math.random() must stay in [0, 1), got {import_random}"
    );
}

#[tokio::test]
async fn convex_semantics_date_now_frozen_for_whole_query_and_mutation_handler() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
globalThis.__nimbusInvoke = async function () {
  const first = Date.now();
  const constructed = new Date().getTime();
  await new Promise((resolve) => setTimeout(resolve, 25));
  const second = Date.now();
  return { first, second, constructed };
};

export {};
"#;
    for kind in [InvocationKind::Query, InvocationKind::Mutation] {
        let result =
            invoke_convex_semantics_bundle(bundle, &request_of_kind(kind.clone(), "messages:list"))
                .await;
        assert_eq!(
            result["first"], result["second"],
            "Date.now() must be frozen for the whole {kind:?} handler"
        );
        assert_eq!(
            result["first"], result["constructed"],
            "new Date() must observe the same frozen clock in a {kind:?}"
        );
        let frozen = result["first"]
            .as_f64()
            .expect("frozen ms should be a number");
        assert!(
            frozen > 1_500_000_000_000.0,
            "frozen clock should be a plausible wall-clock timestamp, got {frozen}"
        );
    }
}

#[tokio::test]
async fn convex_semantics_action_clock_advances_and_random_is_host_backed() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
globalThis.__nimbusInvoke = async function () {
  const first = Date.now();
  await new Promise((resolve) => setTimeout(resolve, 25));
  const second = Date.now();
  return { first, second, randomFirst: Math.random(), randomSecond: Math.random() };
};

export {};
"#;
    let result = invoke_convex_semantics_bundle(
        bundle,
        &request_of_kind(InvocationKind::Action, "messages:send"),
    )
    .await;
    let first = result["first"].as_f64().expect("first should be a number");
    let second = result["second"]
        .as_f64()
        .expect("second should be a number");
    assert!(
        second > first,
        "action Date.now() must advance across a 25ms await (first={first}, second={second})"
    );
    assert_ne!(result["randomFirst"], result["randomSecond"]);
}

#[tokio::test]
async fn convex_semantics_performance_now_fixed_in_query_incrementing_in_mutation() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
globalThis.__nimbusInvoke = async function () {
  const first = performance.now();
  await new Promise((resolve) => setTimeout(resolve, 25));
  const second = performance.now();
  return { first, second };
};

export {};
"#;
    let query_result =
        invoke_convex_semantics_bundle(bundle, &query_request("messages:list")).await;
    assert_eq!(
        query_result["first"], query_result["second"],
        "performance.now() must be fixed during query execution"
    );

    let mutation_result = invoke_convex_semantics_bundle(
        bundle,
        &request_of_kind(InvocationKind::Mutation, "messages:send"),
    )
    .await;
    let first = mutation_result["first"]
        .as_f64()
        .expect("mutation first should be a number");
    let second = mutation_result["second"]
        .as_f64()
        .expect("mutation second should be a number");
    assert!(
        second > first,
        "performance.now() must increment inside mutations (first={first}, second={second})"
    );
}

#[tokio::test]
async fn convex_semantics_fetch_rejected_in_queries_and_mutations_only() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
globalThis.__nimbusInvoke = async function () {
  try {
    await fetch("https://nimbus-guest-semantics.invalid/");
    return { outcome: "fetched" };
  } catch (error) {
    return { outcome: "rejected", message: String(error?.message ?? error) };
  }
};

export {};
"#;
    for kind in [InvocationKind::Query, InvocationKind::Mutation] {
        let result =
            invoke_convex_semantics_bundle(bundle, &request_of_kind(kind.clone(), "messages:list"))
                .await;
        assert_eq!(result["outcome"], "rejected");
        let message = result["message"]
            .as_str()
            .expect("message should be a string");
        assert!(
            message.contains("Can't use fetch() in queries and mutations"),
            "{kind:?} fetch must fail with the actions-only contract error, got: {message}"
        );
    }

    // Actions pass the semantics gate and reach the ordinary egress layer
    // (denied here by the test's coarse permissions, NOT by the actions-only
    // rule — the gate must not swallow actions).
    let action_result = invoke_convex_semantics_bundle(
        bundle,
        &request_of_kind(InvocationKind::Action, "messages:send"),
    )
    .await;
    assert_eq!(action_result["outcome"], "rejected");
    let action_message = action_result["message"]
        .as_str()
        .expect("action message should be a string");
    assert!(
        !action_message.contains("Can't use fetch() in queries and mutations"),
        "action fetch must NOT be blocked by the actions-only rule, got: {action_message}"
    );
}

#[tokio::test]
async fn convex_semantics_process_env_subset_is_capability_gated() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let _env_guard = ScopedProcessEnvVar::set("NIMBUS_GUEST_SEMANTICS_TEST_VAR", "granted-value");
    let bundle = r#"
globalThis.__nimbusInvoke = async function () {
  return {
    processType: typeof process,
    granted: process.env.NIMBUS_GUEST_SEMANTICS_TEST_VAR,
    denied: process.env.NIMBUS_GUEST_SEMANTICS_DENIED_VAR,
    versionsType: typeof process.versions,
  };
};

export {};
"#;
    let (_tempdir, bundle_path) = write_app_style_bundle(bundle);
    let mut limits = convex_semantics_limits();
    limits
        .grants
        .env_read
        .push("NIMBUS_GUEST_SEMANTICS_TEST_VAR".to_string());
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &query_request("messages:list"),
            "tenant-a",
        )
        .await
        .expect("process.env bundle invocation should succeed");

    assert_eq!(result["processType"], "object");
    assert_eq!(result["granted"], "granted-value");
    assert_eq!(result["denied"], Value::Null);
    assert_eq!(
        result["versionsType"], "undefined",
        "the Convex default runtime process is env-only, not a Node process"
    );
}

#[tokio::test]
async fn convex_semantics_async_local_storage_and_async_resource_work() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";
import bareDefault from "async_hooks";

const storage = new AsyncLocalStorage();

globalThis.__nimbusInvoke = async function () {
  const insideRun = await storage.run({ requestId: "r-1" }, async () => {
    await new Promise((resolve) => setTimeout(resolve, 5));
    return storage.getStore()?.requestId ?? null;
  });
  const outsideRun = storage.getStore() === undefined ? "unset" : "leaked";

  const resource = new AsyncResource("nimbus-test");
  let boundObserved = null;
  await storage.run({ requestId: "r-2" }, async () => {
    // The resource snapshot was taken OUTSIDE the store: the bound callback
    // must not observe r-2.
    await new Promise((resolve) => {
      const bound = resource.bind(() => {
        boundObserved = storage.getStore()?.requestId ?? "outside";
        resolve();
      });
      setTimeout(bound, 5);
    });
  });

  return {
    insideRun,
    outsideRun,
    boundObserved,
    bareAliasMatches: bareDefault.AsyncLocalStorage === AsyncLocalStorage,
  };
};

export {};
"#;
    let result = invoke_convex_semantics_bundle(bundle, &query_request("messages:list")).await;
    assert_eq!(
        result["insideRun"], "r-1",
        "AsyncLocalStorage store must propagate across awaited timers inside run()"
    );
    assert_eq!(result["outsideRun"], "unset");
    assert_eq!(
        result["boundObserved"], "outside",
        "AsyncResource.bind must pin the context captured at construction time"
    );
    assert_eq!(result["bareAliasMatches"], true);
}

#[tokio::test]
async fn host_semantics_web_standard_lane_rejects_node_async_hooks() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let bundle = r#"
import { AsyncLocalStorage } from "node:async_hooks";

globalThis.__nimbusInvoke = async function () {
  return { type: typeof AsyncLocalStorage };
};

export {};
"#;
    let (_tempdir, bundle_path) = write_app_style_bundle(bundle);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let error = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &query_request("messages:list"),
            "tenant-a",
        )
        .await
        .expect_err("Host-semantics WebStandard lanes must not serve node:async_hooks");
    assert!(
        error.to_string().contains(
            "node: imports are unavailable under RuntimeCompatibilityTarget::WebStandardIsolate"
        ),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn convex_semantics_webassembly_api_available_with_shared_memory_hardening() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    // (module (func (export "add") (param i32 i32) (result i32)
    //   local.get 0 local.get 1 i32.add))
    let bundle = r#"
const wasmBytes = new Uint8Array([
  0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
  0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f,
  0x03, 0x02, 0x01, 0x00,
  0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00,
  0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
]);

globalThis.__nimbusInvoke = async function () {
  const { module, instance } = await WebAssembly.instantiate(wasmBytes);
  let sharedMemoryError = null;
  try {
    new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true });
  } catch (error) {
    sharedMemoryError = String(error?.message ?? error);
  }
  return {
    sum: instance.exports.add(19, 23),
    moduleIsModule: module instanceof WebAssembly.Module,
    instanceIsInstance: instance instanceof WebAssembly.Instance,
    syncModuleWorks: new WebAssembly.Module(wasmBytes) instanceof WebAssembly.Module,
    sharedMemoryError,
  };
};

export {};
"#;
    let result = invoke_convex_semantics_bundle(bundle, &query_request("messages:list")).await;
    assert_eq!(result["sum"], 42);
    assert_eq!(result["moduleIsModule"], true);
    assert_eq!(result["instanceIsInstance"], true);
    assert_eq!(result["syncModuleWorks"], true);
    assert!(
        result["sharedMemoryError"]
            .as_str()
            .expect("shared memory attempt should throw")
            .contains("Nimbus disables shared WebAssembly memory"),
        "shared-memory hardening must stay intact: {:?}",
        result["sharedMemoryError"]
    );
}
