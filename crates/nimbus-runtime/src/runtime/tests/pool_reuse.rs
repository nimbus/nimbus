use std::rc::Rc;

use deno_core::{JsRuntime, PollEventLoopOptions};

use super::super::realm_lease::RuntimeRealmLeaseController;
use super::*;
use crate::backends::v8::{ReusableV8Runtime, V8RuntimeConstructionMode, V8WorkerRuntimePool};
use crate::execution_plan::RuntimeExecutionPlan;
use crate::host::HostBridgeFuture;
use crate::limits::{
    RuntimeCompatibilityTarget, RuntimeExecutionModel, RuntimeMemoryPressureLevel,
    RuntimeNodeFullRealmReusePolicy, RuntimePoolKind, RuntimeRoutingAffinity,
};

#[tokio::test]
async fn pooled_runtime_invocations_keep_module_state_fresh() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__nimbusInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = invoke_on_single_worker(&executor, runtime.clone(), &bundle, request.clone())
        .await
        .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(&executor, runtime, &bundle, request)
        .await
        .expect("second pooled invocation should succeed");

    assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
    assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}

#[tokio::test]
async fn pooled_runtime_invocations_reset_auth_and_host_call_session_state() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const user = await ctx.auth.getUserIdentity();
  const host = await ctx.db.get("messages", "doc-1");
  return {
    token: user?.tokenIdentifier ?? null,
    session: host.payload.host_call_session_id,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);

    let first = invoke_on_single_worker(
        &executor,
        runtime.clone(),
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:first".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-1")),
            services: Default::default(),
        },
    )
    .await
    .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(
        &executor,
        runtime,
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:second".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-2")),
            services: Default::default(),
        },
    )
    .await
    .expect("second pooled invocation should succeed");

    assert_eq!(
        first,
        serde_json::json!({
            "token": "token-1",
            "session": "query:auth:first",
        })
    );
    assert_eq!(
        second,
        serde_json::json!({
            "token": "token-2",
            "session": "query:auth:second",
        })
    );
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
}

#[derive(Clone)]
struct TaggedAsyncDbGetHost {
    host_id: &'static str,
}

impl HostBridge for TaggedAsyncDbGetHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        _request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let host_id = self.host_id;
        Box::pin(async move {
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "host_id": host_id,
                },
            }))
        })
    }
}

#[tokio::test]
async fn warm_pooled_runtime_rebinds_host_bridge_per_invocation() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = invoke_on_single_worker(
        &executor,
        NimbusRuntime::with_policy(
            Arc::new(TaggedAsyncDbGetHost { host_id: "first" }),
            policy.clone(),
        ),
        &bundle,
        request.clone(),
    )
    .await
    .expect("first warm pooled invocation should succeed");
    let second = invoke_on_single_worker(
        &executor,
        NimbusRuntime::with_policy(Arc::new(TaggedAsyncDbGetHost { host_id: "second" }), policy),
        &bundle,
        request,
    )
    .await
    .expect("second warm pooled invocation should succeed");

    assert_eq!(first, serde_json::json!({ "host_id": "first" }));
    assert_eq!(second, serde_json::json!({ "host_id": "second" }));
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}

#[tokio::test]
async fn warm_context_recycle_reuses_runtime_with_fresh_realm_module_state() {
    let tempdir = tempdir().expect("tempdir should build");
    let dep_path = tempdir.path().join("dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
globalThis.__dependencyLoadCount = (globalThis.__dependencyLoadCount ?? 0) + 1;

export function dependencyLoadCount() {
  return globalThis.__dependencyLoadCount;
}
"#,
    )
    .expect("dependency should write");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import { dependencyLoadCount } from "./dep.mjs";

globalThis.__entryLoadCount = (globalThis.__entryLoadCount ?? 0) + 1;

globalThis.__nimbusInvoke = async function (request) {
  globalThis.__lastFunctionName = request.function_name;
  const ctx = globalThis.__nimbusCreateContext({ request });
  const response = await ctx.db.get("messages", "doc-1");
  return {
    entryLoadCount: globalThis.__entryLoadCount,
    dependencyLoadCount: dependencyLoadCount(),
    lastFunctionName: globalThis.__lastFunctionName,
    response,
  };
};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let bundle = RuntimeBundle::new(&bundle_path);

    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("messages:first");
    let first = executor
        .invoke_on_worker(
            runtime.clone(),
            bundle.clone(),
            first_request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a"),
            None,
        )
        .await
        .expect("first context-recycled invocation should succeed");
    let second_request = request("messages:second");
    let second = executor
        .invoke_on_worker(
            runtime,
            bundle.clone(),
            second_request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a"),
            None,
        )
        .await
        .expect("second context-recycled invocation should succeed");

    assert_eq!(
        first,
        serde_json::json!({
            "entryLoadCount": 1,
            "dependencyLoadCount": 1,
            "lastFunctionName": "messages:first",
            "response": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "host_call_session_id": "query:messages:first",
                },
            },
        })
    );
    assert_eq!(
        second,
        serde_json::json!({
            "entryLoadCount": 1,
            "dependencyLoadCount": 1,
            "lastFunctionName": "messages:second",
            "response": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "host_call_session_id": "query:messages:second",
                },
            },
        })
    );
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}

#[tokio::test]
async fn fresh_realm_driver_destroys_realm_after_success_and_error() {
    let destroy_probe =
        super::super::realm_lifecycle::test_probe::start_fresh_realm_destroy_probe();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  if (request.function_name === "messages:fail") {
    throw new Error("fresh realm failure");
  }
  return {
    ok: true,
    functionName: request.function_name,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let success_request = request("messages:ok");
    let success = executor
        .invoke_with_cancellation(
            runtime.clone(),
            bundle.clone(),
            success_request.clone(),
            RuntimeInvocationContext::top_level(&success_request),
            None,
        )
        .await
        .expect("successful fresh-realm invocation should resolve");
    assert_eq!(
        success,
        serde_json::json!({
            "ok": true,
            "functionName": "messages:ok",
        })
    );
    assert_eq!(
        destroy_probe.count(),
        1,
        "successful fresh-realm invocation should destroy its realm"
    );

    let failing_request = request("messages:fail");
    let error = executor
        .invoke_with_cancellation(
            runtime,
            bundle,
            failing_request.clone(),
            RuntimeInvocationContext::top_level(&failing_request),
            None,
        )
        .await
        .expect_err("failing fresh-realm invocation should preserve the JS error");
    assert!(
        error.to_string().contains("fresh realm failure"),
        "fresh-realm failure should remain visible, got {error}"
    );
    assert_eq!(
        destroy_probe.count(),
        2,
        "failing fresh-realm invocation should also destroy its realm"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_replays_extension_js_before_bundle_load() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return {
    bufferType: typeof globalThis.Buffer,
    bufferRoundTrip: globalThis.Buffer?.from("realm").toString("utf8") ?? null,
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
    globalAliasIsSelf: globalThis.global === globalThis,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:node-realm".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let (value, realm) = runtime_owner
        .start_fresh_realm_bundle_invocation_with_trace(
            &mut runtime,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .await
        .expect("NodeFull fresh realm invocation should start");
    let result = runtime_owner
        .resolve_fresh_realm_invocation_response_with_trace(
            &mut runtime,
            &realm,
            value,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .await
        .expect("NodeFull fresh realm response should resolve");
    super::super::realm_lifecycle::destroy_fresh_realm(&mut runtime, realm);

    assert_eq!(
        result,
        serde_json::json!({
            "bufferType": "function",
            "bufferRoundTrip": "realm",
            "processVersion": "v22.22.3",
            "nodeVersion": "22.22.3",
            "globalAliasIsSelf": true,
        })
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_returns_clean_and_rejects_cross_tenant() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  return {
    functionName: request.function_name,
    mainRealmSentinelType: typeof globalThis.__mainRealmSentinel,
    nodeVersion: globalThis.process?.versions?.node ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    runtime
        .execute_script(
            "<nimbus-runtime:test-main-realm-sentinel>",
            r#"
globalThis.__mainRealmSentinel = "main";
globalThis.__nimbusInvoke = () => ({ mainRealmFallback: true });
"#,
        )
        .expect("main realm sentinel should install");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("messages:first");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first tenant-a lease invocation should return clean");
    assert_eq!(
        first,
        serde_json::json!({
            "functionName": "messages:first",
            "mainRealmSentinelType": "undefined",
            "nodeVersion": "22.22.3",
        })
    );

    let second_request = request("messages:second");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second tenant-a lease invocation should reuse the clean substrate");
    assert_eq!(
        second,
        serde_json::json!({
            "functionName": "messages:second",
            "mainRealmSentinelType": "undefined",
            "nodeVersion": "22.22.3",
        })
    );

    let tenant_b_request = request("messages:tenant-b");
    let tenant_b_context =
        RuntimeInvocationContext::top_level_for_tenant(&tenant_b_request, "tenant-b");
    let tenant_b_error = match runtime_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            &controller,
            &mut runtime,
            &bundle,
            &tenant_b_request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&tenant_b_context),
        )
        .await
    {
        Ok(_) => panic!("tenant-b must not acquire tenant-a's retained substrate"),
        Err(error) => error,
    };
    assert!(
        tenant_b_error.to_string().contains("owner mismatch"),
        "cross-tenant rejection should report the lease owner mismatch, got {tenant_b_error}"
    );

    let third_request = request("messages:third");
    let third_context = RuntimeInvocationContext::top_level_for_tenant(&third_request, "tenant-a");
    let third = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &third_request,
        &third_context,
    )
    .await
    .expect("tenant-a should still reuse the clean substrate after tenant-b rejection");
    assert_eq!(
        third,
        serde_json::json!({
            "functionName": "messages:third",
            "mainRealmSentinelType": "undefined",
            "nodeVersion": "22.22.3",
        })
    );

    let missing_context_controller = RuntimeRealmLeaseController::new(Default::default());
    let missing_context_error = match runtime_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            &missing_context_controller,
            &mut runtime,
            &bundle,
            &third_request,
            V8RuntimeConstructionMode::StartupSnapshot,
            None,
        )
        .await
    {
        Ok(_) => panic!("lease checkout without invocation context must fail closed"),
        Err(error) => error,
    };
    assert!(
        missing_context_error
            .to_string()
            .contains("requires an invocation context"),
        "missing context should be explicit, got {missing_context_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_enforces_target_authority_and_metadata() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  return {
    functionName: request.function_name,
    processVersion: globalThis.process?.version ?? null,
    nodeVersion: globalThis.process?.versions?.node ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    for (target, expected_version) in [
        (RuntimeCompatibilityTarget::Node20, "20.20.2"),
        (RuntimeCompatibilityTarget::Node22, "22.22.3"),
        (RuntimeCompatibilityTarget::Node24, "24.16.0"),
        (RuntimeCompatibilityTarget::Node26, "26.2.0"),
    ] {
        let lane_name = target
            .node_lts_lane_name()
            .expect("test target should be a Node lane");
        let target_owner = NimbusRuntime::with_policy(
            host.clone(),
            Arc::new(RuntimePolicy::new(crate::RuntimeLimits::application_node(
                target,
            ))),
        );
        let snapshot = target_owner
            .bootstrap_snapshot()
            .expect("NodeFull bootstrap snapshot should build");
        let mut runtime = target_owner
            .create_runtime_from_snapshot(&bundle, snapshot)
            .expect("NodeFull runtime should build from snapshot");
        let controller = RuntimeRealmLeaseController::new(Default::default());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: format!("messages:{lane_name}"),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

        let result = invoke_node_full_fresh_realm_with_lease(
            &target_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{target:?}: target-owned Node metadata invocation should succeed: {error}")
        });

        assert_eq!(
            result,
            serde_json::json!({
                "functionName": format!("messages:{lane_name}"),
                "processVersion": format!("v{expected_version}"),
                "nodeVersion": expected_version,
            }),
            "{target:?}: lease realm should expose the active target metadata"
        );
    }

    let node22_owner = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let node24_owner = NimbusRuntime::with_policy(
        host,
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node24(),
        )),
    );
    let snapshot = node22_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = node22_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let node22_request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:node22-authority".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let node22_context =
        RuntimeInvocationContext::top_level_for_tenant(&node22_request, "tenant-a");
    let node22 = invoke_node_full_fresh_realm_with_lease(
        &node22_owner,
        &controller,
        &mut runtime,
        &bundle,
        &node22_request,
        &node22_context,
    )
    .await
    .expect("node22 authority should return clean");
    assert_eq!(
        node22["nodeVersion"],
        serde_json::json!("22.22.3"),
        "node22 lease should expose node22 metadata"
    );

    let node24_request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:node24-authority".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let node24_context =
        RuntimeInvocationContext::top_level_for_tenant(&node24_request, "tenant-a");
    let target_error = match node24_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            &controller,
            &mut runtime,
            &bundle,
            &node24_request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&node24_context),
        )
        .await
    {
        Ok(_) => panic!("node24 authority must not reuse a node22-retained substrate"),
        Err(error) => error,
    };
    assert!(
        target_error.to_string().contains("authority key mismatch"),
        "cross-target reuse should reject at lease checkout, got {target_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_rejects_cross_bundle_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_a_path = tempdir.path().join("bundle-a.mjs");
    let bundle_b_path = tempdir.path().join("bundle-b.mjs");
    let bundle_source = |label: &str| {
        format!(
            r#"
globalThis.__nimbusInvoke = function (request) {{
  return {{
    bundleLabel: "{label}",
    functionName: request.function_name,
  }};
}};

export {{}};
"#
        )
    };
    std::fs::write(&bundle_a_path, bundle_source("a")).expect("bundle A should write");
    std::fs::write(&bundle_b_path, bundle_source("b")).expect("bundle B should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle_a = RuntimeBundle::new(&bundle_a_path);
    let bundle_b = RuntimeBundle::new(&bundle_b_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle_a, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("messages:bundle-a");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle_a,
        &first_request,
        &first_context,
    )
    .await
    .expect("bundle A authority should return clean");
    assert_eq!(
        first,
        serde_json::json!({
            "bundleLabel": "a",
            "functionName": "messages:bundle-a",
        })
    );

    let bundle_b_request = request("messages:bundle-b");
    let bundle_b_context =
        RuntimeInvocationContext::top_level_for_tenant(&bundle_b_request, "tenant-a");
    let bundle_error = match runtime_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            &controller,
            &mut runtime,
            &bundle_b,
            &bundle_b_request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&bundle_b_context),
        )
        .await
    {
        Ok(_) => panic!("bundle B authority must not reuse bundle A's retained substrate"),
        Err(error) => error,
    };
    assert!(
        bundle_error.to_string().contains("authority key mismatch"),
        "cross-bundle reuse should reject at lease checkout, got {bundle_error}"
    );

    let second_request = request("messages:bundle-a-again");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle_a,
        &second_request,
        &second_context,
    )
    .await
    .expect("bundle A authority should remain reusable after rejected bundle B checkout");
    assert_eq!(
        second,
        serde_json::json!({
            "bundleLabel": "a",
            "functionName": "messages:bundle-a-again",
        })
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_preserves_translator_mode_boundary_per_target() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.cjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const namespace = await import("./dep.cjs");
  const hasModuleExportsMarker =
    Object.prototype.hasOwnProperty.call(namespace, "module.exports");
  return {
    keys: Object.keys(namespace).sort(),
    hasModuleExportsMarker,
    defaultMarker: namespace.default?.marker ?? null,
    moduleExportsMarker: namespace["module.exports"]?.marker ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");
    std::fs::write(
        &dep_path,
        r#"
module.exports = {
  marker: "commonjs-default",
};
module.exports.namedValue = 42;
"#,
    )
    .expect("CommonJS dependency should write");

    let host = Arc::new(RecordingHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    for (target, expected_has_marker) in [
        (RuntimeCompatibilityTarget::Node22, false),
        (RuntimeCompatibilityTarget::Node24, true),
    ] {
        let target_owner = NimbusRuntime::with_policy(
            host.clone(),
            Arc::new(RuntimePolicy::new(crate::RuntimeLimits::application_node(
                target,
            ))),
        );
        let snapshot = target_owner
            .bootstrap_snapshot()
            .expect("NodeFull bootstrap snapshot should build");
        let mut runtime = target_owner
            .create_runtime_from_snapshot(&bundle, snapshot)
            .expect("NodeFull runtime should build from snapshot");
        let controller = RuntimeRealmLeaseController::new(Default::default());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: format!(
                "messages:{}",
                target
                    .node_lts_lane_name()
                    .expect("test target should be a Node lane")
            ),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

        let result = invoke_node_full_fresh_realm_with_lease(
            &target_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{target:?}: translator-mode lease invocation should succeed: {error}")
        });

        assert_eq!(
            result["defaultMarker"],
            serde_json::json!("commonjs-default"),
            "{target:?}: default CommonJS import should remain available"
        );
        assert_eq!(
            result["hasModuleExportsMarker"],
            serde_json::json!(expected_has_marker),
            "{target:?}: translator mode should preserve the module.exports marker boundary"
        );
        assert_eq!(
            result["moduleExportsMarker"],
            if expected_has_marker {
                serde_json::json!("commonjs-default")
            } else {
                Value::Null
            },
            "{target:?}: module.exports marker payload should match the active translator mode"
        );
    }
}

#[derive(Default)]
struct ServiceLookupHost {
    async_calls: std::sync::Mutex<Vec<HostCallRequest>>,
}

impl ServiceLookupHost {
    fn calls(&self) -> Vec<HostCallRequest> {
        self.async_calls
            .lock()
            .expect("service lookup async host lock should not be poisoned")
            .clone()
    }
}

impl HostBridge for ServiceLookupHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(format!(
            "unexpected sync host op during NodeFull fresh-realm service lookup test: {}",
            request.operation
        )))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let service_name = request
            .payload
            .get("service_name")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_default();
        self.async_calls
            .lock()
            .expect("service lookup async host lock should not be poisoned")
            .push(request);
        Box::pin(async move {
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "service_name": service_name,
                },
            }))
        })
    }
}

fn node22_policy_with_native_service_grant(service_name: &str) -> Arc<RuntimePolicy> {
    let mut limits = crate::RuntimeLimits::application_node22();
    limits.service_capability_enabled = true;
    limits.grants.service = vec![service_name.to_string()];
    Arc::new(RuntimePolicy::new(limits))
}

#[tokio::test]
async fn node_full_fresh_realm_lease_requires_exact_service_authority_on_retained_substrate() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const serviceName = request.args?.serviceName ?? "db";
  const binding = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_service_lookup", {
    service_name: serviceName,
    host_call_session_id: `action:${request.function_name}`,
  });
  return {
    functionName: request.function_name,
    serviceName,
    binding,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(ServiceLookupHost::default());
    let db_owner =
        NimbusRuntime::with_policy(host.clone(), node22_policy_with_native_service_grant("db"));
    let cache_owner = NimbusRuntime::with_policy(
        host.clone(),
        node22_policy_with_native_service_grant("cache"),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = db_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build with service op");
    let mut runtime = db_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, service_name: &str| InvocationRequest {
        kind: InvocationKind::Action,
        function_name: function_name.to_string(),
        args: serde_json::json!({ "serviceName": service_name }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let db_request = request("services:get-db", "db");
    let db_context = RuntimeInvocationContext::top_level_for_tenant(&db_request, "tenant-a");
    let db_result = invoke_node_full_fresh_realm_with_lease(
        &db_owner,
        &controller,
        &mut runtime,
        &bundle,
        &db_request,
        &db_context,
    )
    .await
    .expect("db service lookup should return a clean retained lease");
    assert_eq!(
        db_result,
        serde_json::json!({
            "functionName": "services:get-db",
            "serviceName": "db",
            "binding": {
                "service_name": "db",
            },
        })
    );
    assert_eq!(
        host.calls().len(),
        1,
        "db lookup should reach the host once"
    );

    let cache_policy_request = request("services:get-cache", "cache");
    let cache_policy_context =
        RuntimeInvocationContext::top_level_for_tenant(&cache_policy_request, "tenant-a");
    let authority_error = match cache_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            &controller,
            &mut runtime,
            &bundle,
            &cache_policy_request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&cache_policy_context),
        )
        .await
    {
        Ok(_) => panic!("cache service authority must not reuse the db-retained substrate"),
        Err(error) => error,
    };
    assert!(
        authority_error
            .to_string()
            .contains("authority key mismatch"),
        "service authority mismatch should reject checkout, got {authority_error}"
    );
    assert_eq!(
        host.calls().len(),
        1,
        "rejected service authority checkout should not reach the host"
    );

    let second_db_request = request("services:get-db-again", "db");
    let second_db_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_db_request, "tenant-a");
    let second_db_result = invoke_node_full_fresh_realm_with_lease(
        &db_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_db_request,
        &second_db_context,
    )
    .await
    .expect("db authority should still reuse after a rejected cache checkout");
    assert_eq!(
        second_db_result,
        serde_json::json!({
            "functionName": "services:get-db-again",
            "serviceName": "db",
            "binding": {
                "service_name": "db",
            },
        })
    );
    assert_eq!(
        host.calls().len(),
        2,
        "second db lookup should reach the host"
    );

    let denied_request = request("services:denied-cache", "cache");
    let denied_context =
        RuntimeInvocationContext::top_level_for_tenant(&denied_request, "tenant-a");
    let denied_error = invoke_node_full_fresh_realm_with_lease(
        &db_owner,
        &controller,
        &mut runtime,
        &bundle,
        &denied_request,
        &denied_context,
    )
    .await
    .expect_err("active db contract should deny an ungranted cache lookup");
    assert!(
        denied_error
            .to_string()
            .contains("runtime service grant denied for `cache`"),
        "unexpected denied service lookup error: {denied_error}"
    );
    assert_eq!(
        host.calls().len(),
        2,
        "denied service lookup should not reach the host bridge"
    );
}

fn assert_coarsened_timer_samples(samples: &Value, label: &str) {
    let samples = samples
        .as_array()
        .unwrap_or_else(|| panic!("{label} samples should serialize as an array: {samples}"));
    assert!(!samples.is_empty(), "{label} should produce samples");
    for sample in samples {
        let value = sample
            .as_f64()
            .unwrap_or_else(|| panic!("{label} sample should be numeric: {sample}"));
        assert_eq!(
            value.rem_euclid(10.0),
            0.0,
            "{label} sample {value} should be coarsened to 10ms buckets"
        );
    }
}

#[tokio::test]
async fn node_full_fresh_realm_lease_applies_side_channel_hardening_per_realm() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
function assertAtomicsWaitDisabled(name) {
  if (typeof Atomics?.[name] !== "function") {
    return { available: false, threw: null, name: null, message: null };
  }
  try {
    Atomics[name](new Int32Array(new ArrayBuffer(4)), 0, 0, 0);
    return { available: true, threw: false, name: null, message: null };
  } catch (error) {
    return {
      available: true,
      threw: true,
      name: error?.name ?? null,
      message: error?.message ?? String(error),
    };
  }
}

globalThis.__nimbusInvoke = function () {
  return {
    sharedArrayBufferType: typeof globalThis.SharedArrayBuffer,
    atomicsWaitType: typeof Atomics?.wait,
    atomicsWait: assertAtomicsWaitDisabled("wait"),
    atomicsWaitAsyncType: typeof Atomics?.waitAsync,
    atomicsWaitAsync: assertAtomicsWaitDisabled("waitAsync"),
    dateNowSamples: Array.from({ length: 4 }, () => Date.now()),
    performanceNowSamples: Array.from({ length: 4 }, () => globalThis.performance.now()),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    for target in [
        RuntimeCompatibilityTarget::Node20,
        RuntimeCompatibilityTarget::Node22,
        RuntimeCompatibilityTarget::Node24,
        RuntimeCompatibilityTarget::Node26,
    ] {
        let runtime_owner = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            Arc::new(RuntimePolicy::new(crate::RuntimeLimits::application_node(
                target,
            ))),
        );
        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("NodeFull bootstrap snapshot should build");
        let mut runtime = runtime_owner
            .create_runtime_from_snapshot(&bundle, snapshot)
            .expect("NodeFull runtime should build from snapshot");
        let controller = RuntimeRealmLeaseController::new(Default::default());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: format!(
                "side-channel:{}",
                target
                    .node_lts_lane_name()
                    .expect("test target should be a Node lane")
            ),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

        let result = invoke_node_full_fresh_realm_with_lease(
            &runtime_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{target:?}: side-channel hardened lease invocation should succeed: {error}")
        });

        assert_eq!(
            result["sharedArrayBufferType"],
            serde_json::json!("undefined"),
            "{target:?}: SharedArrayBuffer should be hidden"
        );
        assert_eq!(result["atomicsWaitType"], serde_json::json!("function"));
        assert_eq!(result["atomicsWait"]["available"], serde_json::json!(true));
        assert_eq!(result["atomicsWait"]["threw"], serde_json::json!(true));
        assert!(
            result["atomicsWait"]["message"]
                .as_str()
                .expect("Atomics.wait error should serialize a message")
                .contains("Nimbus disables Atomics.wait"),
            "{target:?}: unexpected Atomics.wait error: {}",
            result["atomicsWait"]
        );
        if result["atomicsWaitAsync"]["available"] == serde_json::json!(true) {
            assert_eq!(result["atomicsWaitAsync"]["threw"], serde_json::json!(true));
            assert!(
                result["atomicsWaitAsync"]["message"]
                    .as_str()
                    .expect("Atomics.waitAsync error should serialize a message")
                    .contains("Nimbus disables Atomics.waitAsync"),
                "{target:?}: unexpected Atomics.waitAsync error: {}",
                result["atomicsWaitAsync"]
            );
        }
        assert_coarsened_timer_samples(&result["dateNowSamples"], "Date.now");
        assert_coarsened_timer_samples(&result["performanceNowSamples"], "performance.now");
    }
}

#[tokio::test]
async fn node_full_fresh_realm_lease_denies_inspector_and_repl_in_production() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
async function capture(specifier) {
  try {
    await import(specifier);
    return { ok: true, message: null };
  } catch (error) {
    return { ok: false, message: error?.message ?? String(error) };
  }
}

globalThis.__nimbusInvoke = async function () {
  return {
    inspector: await capture("node:inspector"),
    repl: await capture("node:repl"),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "authority:debug-surfaces".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let result = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &request,
        &context,
    )
    .await
    .expect("debug-surface denial probe should execute");

    assert_eq!(result["inspector"]["ok"], serde_json::json!(false));
    assert!(
        result["inspector"]["message"]
            .as_str()
            .expect("inspector denial should serialize a message")
            .contains("inspector authority"),
        "unexpected inspector denial: {}",
        result["inspector"]
    );
    assert_eq!(result["repl"]["ok"], serde_json::json!(false));
    assert!(
        result["repl"]["message"]
            .as_str()
            .expect("REPL denial should serialize a message")
            .contains("REPL authority"),
        "unexpected REPL denial: {}",
        result["repl"]
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_matches_startup_snapshot_for_node_fixture() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.cjs");
    std::fs::write(
        &bundle_path,
        r#"
import { Buffer } from "node:buffer";
import path from "node:path";

globalThis.__nimbusInvoke = async function () {
  const namespace = await import("./dep.cjs");
  return {
    bufferHex: Buffer.from("realm").toString("hex"),
    basename: path.basename("/tmp/nimbus/realm.txt"),
    nodeVersion: globalThis.process?.versions?.node ?? null,
    commonJsDefault: namespace.default?.marker ?? null,
    hasModuleExportsMarker:
      Object.prototype.hasOwnProperty.call(namespace, "module.exports"),
  };
};

export {};
"#,
    )
    .expect("bundle should write");
    std::fs::write(
        &dep_path,
        r#"
module.exports = { marker: "commonjs-default" };
"#,
    )
    .expect("CommonJS dependency should write");
    let bundle = RuntimeBundle::new(&bundle_path);

    for target in [
        RuntimeCompatibilityTarget::Node22,
        RuntimeCompatibilityTarget::Node24,
    ] {
        let runtime_owner = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            Arc::new(RuntimePolicy::new(crate::RuntimeLimits::application_node(
                target,
            ))),
        );
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: format!(
                "parity:{}",
                target
                    .node_lts_lane_name()
                    .expect("test target should be a Node lane")
            ),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };

        let startup_snapshot_result = runtime_owner
            .invoke_bundle(&bundle, &request)
            .await
            .unwrap_or_else(|error| {
                panic!("{target:?}: startup-snapshot invocation should succeed: {error}")
            });

        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("NodeFull bootstrap snapshot should build");
        let mut runtime = runtime_owner
            .create_runtime_from_snapshot(&bundle, snapshot)
            .expect("NodeFull runtime should build from snapshot");
        let controller = RuntimeRealmLeaseController::new(Default::default());
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let lease_result = invoke_node_full_fresh_realm_with_lease(
            &runtime_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{target:?}: fresh-realm lease invocation should succeed: {error}")
        });

        assert_eq!(
            lease_result, startup_snapshot_result,
            "{target:?}: fresh-realm lease should match startup-snapshot semantics"
        );
    }
}

#[tokio::test]
async fn node_full_fresh_realm_lease_denies_query_host_effects_before_dispatch() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
async function capture(fn) {
  try {
    await fn();
    return null;
  } catch (error) {
    return error?.message ?? String(error);
  }
}

globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  return {
    insertError: await capture(() => ctx.db.insert("messages", { body: "blocked" })),
    schedulerError: await capture(() => ctx.scheduler.runAfter(1, "messages:send", {})),
    nestedMutationError: await capture(() => ctx.runMutation("messages:send", {})),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime_owner = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "effects:query-denials".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let result = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &request,
        &context,
    )
    .await
    .expect("query host-effect denial probe should execute");

    for field in ["insertError", "schedulerError", "nestedMutationError"] {
        let message = result[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} should serialize an error string: {result}"));
        assert!(
            message.contains("not available for query handlers"),
            "unexpected {field}: {message}"
        );
    }
    assert!(
        host.calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .is_empty(),
        "query-shaped host-effect denials should not dispatch to the host bridge"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_resets_opstate_auth_host_session_and_globals() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const previousToken = globalThis.__lastObservedToken ?? null;
  const ctx = globalThis.__nimbusCreateContext({ request });
  const user = await ctx.auth.getUserIdentity();
  globalThis.__lastObservedToken = user?.tokenIdentifier ?? null;
  const host = await ctx.db.get("messages", "doc-1");
  return {
    token: user?.tokenIdentifier ?? null,
    previousToken,
    hostSession: host.payload.host_call_session_id,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, token: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: Some(test_invocation_auth(token)),
        services: Default::default(),
    };

    let first_request = request("auth:first", "token-1");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first auth/session lease invocation should succeed");
    assert_eq!(
        first,
        serde_json::json!({
            "token": "token-1",
            "previousToken": null,
            "hostSession": "query:auth:first",
        })
    );

    let second_request = request("auth:second", "token-2");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second auth/session lease invocation should reuse cleanly");
    assert_eq!(
        second,
        serde_json::json!({
            "token": "token-2",
            "previousToken": null,
            "hostSession": "query:auth:second",
        }),
        "auth, host-call session, and realm globals must be rebound per lease"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_dirty_invocation_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  if (request.args?.mode === "throw") {
    throw new Error("dirty invocation failure");
  }
  return { mode: request.args?.mode ?? "ok" };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, mode: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({ "mode": mode }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let clean_request = request("dirty:clean", "clean");
    let clean_context = RuntimeInvocationContext::top_level_for_tenant(&clean_request, "tenant-a");
    let clean = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &clean_request,
        &clean_context,
    )
    .await
    .expect("initial clean invocation should retain the substrate");
    assert_eq!(clean, serde_json::json!({ "mode": "clean" }));

    let dirty_request = request("dirty:throw", "throw");
    let dirty_context = RuntimeInvocationContext::top_level_for_tenant(&dirty_request, "tenant-a");
    let dirty_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &dirty_request,
        &dirty_context,
    )
    .await
    .expect_err("throwing invocation should condemn the lease");
    assert!(
        dirty_error.to_string().contains("dirty invocation failure"),
        "unexpected dirty invocation error: {dirty_error}"
    );

    let reuse_request = request("dirty:reuse", "reuse");
    let reuse_context = RuntimeInvocationContext::top_level_for_tenant(&reuse_request, "tenant-a");
    let reuse_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &reuse_request,
        &reuse_context,
    )
    .await
    .expect_err("condemned substrate must not be reused by the same authority");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: Dirty"),
        "unexpected condemned-substrate error: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_rejected_wait_until_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  if (request.args?.mode === "reject-background") {
    globalThis.__nimbusWaitUntil(Promise.reject(new Error("background rejected")));
  } else {
    globalThis.__nimbusWaitUntil(Promise.resolve("background ok"));
  }
  return { mode: request.args?.mode ?? "resolve-background" };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, mode: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({ "mode": mode }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let resolved_request = request("wait-until:resolved", "resolve-background");
    let resolved_context =
        RuntimeInvocationContext::top_level_for_tenant(&resolved_request, "tenant-a");
    let resolved = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &resolved_request,
        &resolved_context,
    )
    .await
    .expect("resolved waitUntil work should drain and return clean");
    assert_eq!(
        resolved,
        serde_json::json!({ "mode": "resolve-background" })
    );

    let rejected_request = request("wait-until:rejected", "reject-background");
    let rejected_context =
        RuntimeInvocationContext::top_level_for_tenant(&rejected_request, "tenant-a");
    let rejected_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &rejected_request,
        &rejected_context,
    )
    .await
    .expect_err("rejected waitUntil work should condemn the lease");
    assert!(
        rejected_error
            .to_string()
            .contains("Nimbus waitUntil background drain rejected 1 promise"),
        "unexpected waitUntil rejection error: {rejected_error}"
    );

    let reuse_request = request("wait-until:reuse", "resolve-background");
    let reuse_context = RuntimeInvocationContext::top_level_for_tenant(&reuse_request, "tenant-a");
    let reuse_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &reuse_request,
        &reuse_context,
    )
    .await
    .expect_err("substrate with rejected waitUntil work must not be reused");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: Dirty"),
        "unexpected condemned-substrate error after waitUntil rejection: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_stalled_wait_until_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  globalThis.__nimbusWaitUntil(new Promise(() => {}));
  return { scheduled: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let system_timeout = ci_or_local_duration(
        std::time::Duration::from_millis(50),
        std::time::Duration::from_millis(300),
    );
    let mut limits = crate::RuntimeLimits::application_node22();
    limits.execution_timeout = std::time::Duration::from_secs(5);
    limits.system_timeout = system_timeout;
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "wait-until:stalled".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);
    let controller = reusable_runtime.realm_lease_controller.clone();

    let (result, reusable_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        reusable_runtime,
        &bundle,
        &request,
        &context,
        None,
    )
    .await;
    match result.expect_err("stalled waitUntil work should hit the system timeout") {
        NimbusRuntimeError::SystemTimeout(timeout) => assert_eq!(timeout, system_timeout),
        other => panic!("unexpected stalled waitUntil error: {other}"),
    }
    assert!(
        reusable_runtime.is_none(),
        "stalled waitUntil background work must not return a reusable NodeFull substrate"
    );

    let reuse_error = runtime_owner
        .checkout_fresh_realm_lease(
            &controller,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .expect_err("stalled waitUntil substrate must reject later same-authority checkout");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: TimedOut"),
        "unexpected stalled waitUntil substrate rejection: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_abandons_uncertain_cleanup_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { cleanResponse: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "cleanup:abandoned".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let policy = runtime_owner.policy();
    let mut permit = SharedInvocationPermit::new(policy.clone(), None, None, true, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit NodeFull fresh-realm invocation");
    let execution_plan = RuntimeExecutionPlan::for_invocation(policy.as_ref(), &request, &context);
    bootstrap::reset_runtime_invocation_state(
        &mut runtime,
        permit.clone(),
        Some(&context),
        Some(&execution_plan),
    );

    {
        let (value, realm, mut lease) = runtime_owner
            .start_fresh_realm_bundle_invocation_with_lease_and_trace(
                &controller,
                &mut runtime,
                &bundle,
                &request,
                V8RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
            )
            .await
            .expect("fresh-realm invocation should start");
        let response = runtime_owner
            .resolve_fresh_realm_invocation_response_with_lease_and_trace(
                &mut runtime,
                &realm,
                value,
                &bundle,
                &request,
                V8RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
                &mut lease,
            )
            .await
            .expect("fresh-realm invocation should resolve");
        assert_eq!(response, serde_json::json!({ "cleanResponse": true }));
        super::super::realm_lifecycle::destroy_fresh_realm(&mut runtime, realm);
        drop(lease);
    }

    assert!(
        permit.finish_invocation().await.is_empty(),
        "abandoned NodeFull fresh-realm lease probe should not leave ready jobs"
    );
    let reuse_error = runtime_owner
        .checkout_fresh_realm_lease(
            &controller,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .expect_err("abandoned uncertain-cleanup substrate must reject later checkout");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: Abandoned"),
        "unexpected abandoned-substrate error: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_execution_timeout_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  while (true) {}
};

export {};
"#,
    )
    .expect("bundle should write");

    let execution_timeout = ci_or_local_duration(
        std::time::Duration::from_millis(100),
        std::time::Duration::from_millis(500),
    );
    let mut limits = crate::RuntimeLimits::application_node22();
    limits.execution_timeout = execution_timeout;
    limits.system_timeout = std::time::Duration::from_secs(5);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "timeout:loop".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);
    let controller = reusable_runtime.realm_lease_controller.clone();

    let (result, reusable_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        reusable_runtime,
        &bundle,
        &request,
        &context,
        None,
    )
    .await;
    let error = result.expect_err("infinite loop should time out");
    match error {
        NimbusRuntimeError::ExecutionTimeout(timeout) => {
            assert_eq!(timeout, execution_timeout);
        }
        other => panic!("unexpected timeout error: {other}"),
    }
    assert!(
        reusable_runtime.is_none(),
        "timed-out NodeFull lease substrate must not be returned to the runtime pool"
    );

    let reuse_error = runtime_owner
        .checkout_fresh_realm_lease(
            &controller,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .expect_err("timed-out substrate must reject later same-authority checkout");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: TimedOut"),
        "unexpected timed-out substrate rejection: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_external_cancellation_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  while (true) {}
};

export {};
"#,
    )
    .expect("bundle should write");

    let cancel_after = ci_or_local_duration(
        std::time::Duration::from_millis(100),
        std::time::Duration::from_millis(500),
    );
    let mut limits = crate::RuntimeLimits::application_node22();
    limits.execution_timeout = std::time::Duration::from_secs(5);
    limits.system_timeout = std::time::Duration::from_secs(5);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "cancel:loop".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);
    let controller = reusable_runtime.realm_lease_controller.clone();
    let cancellation = HostCallCancellation::default();
    let cancellation_for_thread = cancellation.clone();
    let cancel_thread = std::thread::spawn(move || {
        std::thread::sleep(cancel_after);
        cancellation_for_thread.cancel();
    });

    let (result, reusable_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        reusable_runtime,
        &bundle,
        &request,
        &context,
        Some(cancellation),
    )
    .await;
    cancel_thread
        .join()
        .expect("cancellation thread should finish cleanly");
    assert!(matches!(result, Err(NimbusRuntimeError::Cancelled)));
    assert!(
        reusable_runtime.is_none(),
        "externally canceled NodeFull lease substrate must not be returned to the runtime pool"
    );

    let reuse_error = runtime_owner
        .checkout_fresh_realm_lease(
            &controller,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .expect_err("externally canceled substrate must reject later same-authority checkout");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: ExternalPressure"),
        "unexpected externally canceled substrate rejection: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_heap_limit_before_reuse() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  await Promise.resolve();
  let value = "";
  while (true) {
    value += "hello world";
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = crate::RuntimeLimits::application_node22();
    limits.initial_heap_mb = 32;
    limits.max_heap_mb = 64;
    limits.execution_timeout = std::time::Duration::from_secs(5);
    limits.system_timeout = std::time::Duration::from_secs(5);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "heap:grow".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);
    let controller = reusable_runtime.realm_lease_controller.clone();

    let (result, reusable_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        reusable_runtime,
        &bundle,
        &request,
        &context,
        None,
    )
    .await;
    let error = result.expect_err("heap growth should trip the near-heap-limit callback");
    match error {
        NimbusRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 64),
        other => panic!("unexpected heap-limit error: {other}"),
    }
    assert!(
        reusable_runtime.is_none(),
        "heap-limited NodeFull lease substrate must not be returned to the runtime pool"
    );

    let reuse_error = runtime_owner
        .checkout_fresh_realm_lease(
            &controller,
            &bundle,
            &request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(&context),
        )
        .expect_err("heap-limited substrate must reject later same-authority checkout");
    assert!(
        reuse_error
            .to_string()
            .contains("realm substrate is condemned: ExternalPressure"),
        "unexpected heap-limited substrate rejection: {reuse_error}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_drops_untracked_timer_host_work() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  if (request.args?.mode === "schedule-timer") {
    const ctx = globalThis.__nimbusCreateContext({ request });
    setTimeout(() => {
      ctx.db.get("messages", "late-timer-host-call").catch(() => {});
    }, 0);
    return { mode: "schedule-timer" };
  }
  await new Promise((resolve) => setTimeout(resolve, 0));
  return { mode: "pump-next-realm" };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime_owner = NimbusRuntime::with_policy(
        host.clone(),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, mode: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({ "mode": mode }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("timer:schedule", "schedule-timer");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("timer scheduling lease invocation should return clean");
    assert_eq!(first, serde_json::json!({ "mode": "schedule-timer" }));
    assert!(
        host.calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .is_empty(),
        "untracked timer host work must not run before clean lease return"
    );

    let second_request = request("timer:pump", "pump-next-realm");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("next lease invocation should reuse cleanly after dropping timer work");
    assert_eq!(second, serde_json::json!({ "mode": "pump-next-realm" }));
    assert!(
        host.calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .is_empty(),
        "untracked timer from a destroyed lease realm must not dispatch host effects later"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_denies_process_and_worker_resource_surfaces_cleanly() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import { spawnSync } from "node:child_process";
import { Worker } from "node:worker_threads";

function captureChildProcess() {
  try {
    const child = spawnSync(process.execPath, ["-e", "console.log('child-ok')"], {
      encoding: "utf8",
    });
    return {
      message: child.error?.message ?? null,
      status: child.status ?? null,
      stdout: child.stdout ?? null,
      stderr: child.stderr ?? null,
    };
  } catch (error) {
    return {
      message: error?.message ?? String(error),
      status: null,
      stdout: null,
      stderr: null,
    };
  }
}

function captureWorkerThread() {
  try {
    new Worker("require('node:worker_threads').parentPort.postMessage('ok')", {
      eval: true,
    });
    return { message: null };
  } catch (error) {
    return { message: error?.message ?? String(error) };
  }
}

function captureFatalProcessControls() {
  const controls = {};
  for (const name of ["abort", "kill"]) {
    const control = process[name];
    const guarded = typeof control === "function" &&
      control.__nimbusDeniedProcessFatalOperation === name;
    let message = null;
    if (guarded) {
      try {
        if (name === "kill") {
          control(process.pid);
        } else {
          control();
        }
      } catch (error) {
        message = error?.message ?? String(error);
      }
    }
    controls[name] = { guarded, message };
  }
  return controls;
}

globalThis.__nimbusInvoke = function (request) {
  return {
    functionName: request.function_name,
    childProcess: captureChildProcess(),
    workerThread: captureWorkerThread(),
    processFatalControls: captureFatalProcessControls(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for function_name in ["resources:first", "resources:second"] {
        let request = request(function_name);
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let result = invoke_node_full_fresh_realm_with_lease(
            &runtime_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{function_name}: resource-surface denial probe should execute: {error}")
        });

        assert_eq!(result["functionName"], serde_json::json!(function_name));
        let child_message = result["childProcess"]["message"].as_str();
        let child_status_denied = result["childProcess"]["status"] == serde_json::json!(null);
        let child_stdout_empty = result["childProcess"]["stdout"].is_null()
            || result["childProcess"]["stdout"] == serde_json::json!("");
        let child_stderr_empty = result["childProcess"]["stderr"].is_null()
            || result["childProcess"]["stderr"] == serde_json::json!("");
        assert!(
            child_message.is_some_and(|message| {
                message.contains("runtime run capability denied")
                    || message.contains("Requires run access")
            }) || (child_status_denied && child_stdout_empty && child_stderr_empty),
            "{function_name}: unexpected child_process denial payload: {result}"
        );
        assert_eq!(result["childProcess"]["status"], serde_json::json!(null));

        let worker_message = result["workerThread"]["message"]
            .as_str()
            .expect("worker creation should be denied by grants");
        assert!(
            worker_message.contains("runtime worker grant denied for `thread`"),
            "{function_name}: unexpected worker denial: {worker_message}"
        );

        for control_name in ["abort", "kill"] {
            let control = &result["processFatalControls"][control_name];
            assert_eq!(
                control["guarded"],
                serde_json::json!(true),
                "{function_name}: process.{control_name} must be guarded before it can abort or signal the host process: {result}"
            );
            let message = control["message"]
                .as_str()
                .expect("guarded fatal process control should throw");
            assert!(
                message.contains(&format!("Nimbus denies process.{control_name}()")),
                "{function_name}: unexpected process.{control_name} denial: {message}"
            );
        }
    }
}

#[tokio::test]
async fn node_full_fresh_realm_lease_rejects_direct_core_host_op_forgery() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
async function captureDirectGet(payload) {
  try {
    return {
      ok: true,
      value: await globalThis.__nimbusHiddenDenoGlobals.core.ops
        .op_nimbus_document_get(payload),
    };
  } catch (error) {
    return {
      ok: false,
      message: error?.message ?? String(error),
    };
  }
}

globalThis.__nimbusInvoke = async function (request) {
  const ops = globalThis.__nimbusHiddenDenoGlobals.core.ops;
  const previousSession = globalThis.__nfr5DirectCoreSession ?? null;
  const currentSession = ops.op_nimbus_runtime_host_call_session_id();
  globalThis.__nfr5DirectCoreSession = currentSession;
  const forged = await captureDirectGet({
    table: "messages",
    id: "doc-1",
    host_call_session_id: "forged-session",
  });
  const missing = await captureDirectGet({
    table: "messages",
    id: "doc-1",
  });
  const current = await captureDirectGet({
    table: "messages",
    id: "doc-1",
    host_call_session_id: currentSession,
  });
    return {
    functionName: request.function_name,
    previousSession,
    currentSession,
    forged,
    missing,
    currentOk: current.ok,
    currentHostSession: current.value?.value?.payload?.host_call_session_id ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for function_name in ["direct-core:first", "direct-core:second"] {
        let request = request(function_name);
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let result = invoke_node_full_fresh_realm_with_lease(
            &runtime_owner,
            &controller,
            &mut runtime,
            &bundle,
            &request,
            &context,
        )
        .await
        .unwrap_or_else(|error| {
            panic!("{function_name}: direct hidden-core host-op probe should execute: {error}")
        });

        let expected_session = format!("query:{function_name}");
        assert_eq!(result["functionName"], serde_json::json!(function_name));
        assert_eq!(result["previousSession"], serde_json::json!(null));
        assert_eq!(
            result["currentSession"],
            serde_json::json!(expected_session)
        );
        assert_eq!(result["currentOk"], serde_json::json!(true));
        assert_eq!(
            result["currentHostSession"],
            serde_json::json!(expected_session)
        );
        for field in ["forged", "missing"] {
            assert_eq!(result[field]["ok"], serde_json::json!(false));
            let message = result[field]["message"]
                .as_str()
                .unwrap_or_else(|| panic!("{function_name}: {field} should return message"));
            assert!(
                message.contains("stale or forged"),
                "{function_name}: unexpected {field} rejection: {message}"
            );
        }
    }
}

#[tokio::test]
async fn node_full_fresh_realm_lease_condemns_live_deno_resource_table_entries() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  if (request.args?.leakResource) {
    const rid = globalThis.__nimbusHiddenDenoGlobals.core.ops.op_cancel_handle();
    globalThis.__nfr5LeakedResourceRid = rid;
    return { leakedRid: rid };
  }
  return { leakedRid: null };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, leak_resource: bool| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({ "leakResource": leak_resource }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("resource-table:first", true);
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect_err("live Deno resources must prevent a clean lease return");
    let first_message = first_error.to_string();
    assert!(
        first_message.contains("changed Deno resource table entries")
            && first_message.contains("cancellation"),
        "unexpected resource-table condemnation error: {first_message}"
    );

    let second_request = request("resource-table:second", false);
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second_error = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect_err("condemned retained substrate must reject later checkout");
    let second_message = second_error.to_string();
    assert!(
        second_message.contains("realm substrate is condemned: Dirty"),
        "unexpected condemned substrate checkout error: {second_message}"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_resets_env_path_and_load_env_file_state() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(tempdir.path().join("config.txt"), "realm-config").expect("config should write");
    std::fs::write(
        tempdir.path().join("first.env"),
        "NFR5_DOTENV_VALUE=dotenv-first\n",
    )
    .expect("first dotenv file should write");
    std::fs::write(
        &bundle_path,
        r#"
import { readFile, stat, writeFile } from "node:fs/promises";

function captureDenied(action) {
  return action().then(
    () => null,
    (error) => error?.message ?? String(error),
  );
}

globalThis.__nimbusInvoke = async function (request) {
  const previousGlobal = globalThis.__nfr5EnvPathMarker ?? null;
  const previousEnv = process.env.NFR5_REALM_ENV ?? null;
  const previousDotenv = process.env.NFR5_DOTENV_VALUE ?? null;
  const config = await readFile("./config.txt", "utf8");
  const writeDenied = await captureDenied(() =>
    writeFile(`../escape-${request.args.label}.txt`, "should-fail")
  );
  const metadataDenied = await captureDenied(() => stat("/"));
  if (request.args.loadDotenv) {
    process.loadEnvFile("./first.env");
  }
  process.env.NFR5_REALM_ENV = request.args.envValue;
  globalThis.__nfr5EnvPathMarker = request.args.label;
  return {
    label: request.args.label,
    previousGlobal,
    previousEnv,
    previousDotenv,
    currentEnv: process.env.NFR5_REALM_ENV ?? null,
    currentDotenv: process.env.NFR5_DOTENV_VALUE ?? null,
    config,
    writeDenied,
    metadataDenied,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = crate::RuntimeLimits::application_node22();
    limits.grants.env_read.push("NFR5_REALM_ENV".to_string());
    limits.grants.env_read.push("NFR5_DOTENV_VALUE".to_string());
    limits.grants.env_write.push("NFR5_REALM_ENV".to_string());
    limits
        .grants
        .env_write
        .push("NFR5_DOTENV_VALUE".to_string());
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request =
        |function_name: &str, label: &str, env_value: &str, load_dotenv: bool| InvocationRequest {
            kind: InvocationKind::Query,
            function_name: function_name.to_string(),
            args: serde_json::json!({
                "label": label,
                "envValue": env_value,
                "loadDotenv": load_dotenv,
            }),
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };

    let first_request = request("env-path:first", "first", "first-env", true);
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first env/path lease invocation should return clean");
    assert_eq!(first["label"], serde_json::json!("first"));
    assert_eq!(first["previousGlobal"], serde_json::json!(null));
    assert_eq!(first["previousEnv"], serde_json::json!(null));
    assert_eq!(first["previousDotenv"], serde_json::json!(null));
    assert_eq!(first["currentEnv"], serde_json::json!("first-env"));
    assert_eq!(first["currentDotenv"], serde_json::json!("dotenv-first"));
    assert_eq!(first["config"], serde_json::json!("realm-config"));

    let second_request = request("env-path:second", "second", "second-env", false);
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second env/path lease invocation should reuse cleanly");
    assert_eq!(
        second,
        serde_json::json!({
            "label": "second",
            "previousGlobal": null,
            "previousEnv": null,
            "previousDotenv": null,
            "currentEnv": "second-env",
            "currentDotenv": null,
            "config": "realm-config",
            "writeDenied": second["writeDenied"].clone(),
            "metadataDenied": second["metadataDenied"].clone(),
        }),
        "clean retained NodeFull lease must not carry env, dotenv, or global state into the next realm"
    );
    for (label, result) in [("first", &first), ("second", &second)] {
        let write_denied = result["writeDenied"]
            .as_str()
            .expect("escape write should be denied");
        assert!(
            write_denied.contains("EACCES")
                || write_denied.contains("runtime write capability denied")
                || write_denied.contains("Requires write access"),
            "{label}: unexpected write denial: {write_denied}"
        );
        let metadata_denied = result["metadataDenied"]
            .as_str()
            .expect("root metadata read should be denied");
        assert!(
            metadata_denied.contains("EACCES")
                || metadata_denied.contains("runtime read capability denied")
                || metadata_denied.contains("Requires read access"),
            "{label}: unexpected metadata denial: {metadata_denied}"
        );
    }
    assert!(
        !tempdir.path().join("escape-first.txt").exists(),
        "first escape write must not materialize outside the generated root"
    );
    assert!(
        !tempdir.path().join("escape-second.txt").exists(),
        "second escape write must not materialize outside the generated root"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_resets_arraybuffer_and_structured_clone_state() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  const previousMarker = globalThis.__nfr5StructuredCloneMarker ?? null;
  const previousDetachedLength = globalThis.__nfr5DetachedLength ?? null;
  const buffer = new ArrayBuffer(16);
  const view = new Uint8Array(buffer);
  view[0] = request.args.byte;
  const cloned = structuredClone({ view }, { transfer: [buffer] });
  globalThis.__nfr5StructuredCloneMarker = request.args.label;
  globalThis.__nfr5DetachedLength = buffer.byteLength;
  return {
    label: request.args.label,
    previousMarker,
    previousDetachedLength,
    sourceBufferDetached: buffer.byteLength === 0,
    detachedLength: buffer.byteLength,
    sourceViewLength: view.byteLength,
    clonedByte: cloned.view[0],
    clonedLength: cloned.view.buffer.byteLength,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, label: &str, byte: u8| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({
            "label": label,
            "byte": byte,
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("clone:first", "first", 17);
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first structured-clone lease invocation should return clean");
    assert_eq!(
        first,
        serde_json::json!({
            "label": "first",
            "previousMarker": null,
            "previousDetachedLength": null,
            "sourceBufferDetached": true,
            "detachedLength": 0,
            "sourceViewLength": 0,
            "clonedByte": 17,
            "clonedLength": 16,
        })
    );

    let second_request = request("clone:second", "second", 29);
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second structured-clone lease invocation should reuse cleanly");
    assert_eq!(
        second,
        serde_json::json!({
            "label": "second",
            "previousMarker": null,
            "previousDetachedLength": null,
            "sourceBufferDetached": true,
            "detachedLength": 0,
            "sourceViewLength": 0,
            "clonedByte": 29,
            "clonedLength": 16,
        }),
        "ArrayBuffer backing-store state and realm globals must not leak across clean NodeFull leases"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_resets_shared_worker_env_helper_state() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
const NAME = "NFR5_SHARED_WORKER_ENV";

globalThis.__nimbusInvoke = function (request) {
  const previousGlobal = globalThis.__nfr5SharedWorkerEnvMarker ?? null;
  const sharedEnv = globalThis.__nimbusInstallSharedWorkerEnvProxy();
  const previousShared = sharedEnv[NAME] ?? null;
  sharedEnv[NAME] = request.args.value;
  globalThis.__nfr5SharedWorkerEnvMarker = request.args.label;
  return {
    label: request.args.label,
    previousGlobal,
    previousShared,
    currentShared: sharedEnv[NAME] ?? null,
    processEnvValue: process.env[NAME] ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = crate::RuntimeLimits::application_node22();
    limits
        .grants
        .env_read
        .push("NFR5_SHARED_WORKER_ENV".to_string());
    limits
        .grants
        .env_write
        .push("NFR5_SHARED_WORKER_ENV".to_string());
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, label: &str, value: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({
            "label": label,
            "value": value,
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("shared-worker-env:first", "first", "first-shared");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first shared-worker-env lease invocation should return clean");
    assert_eq!(
        first,
        serde_json::json!({
            "label": "first",
            "previousGlobal": null,
            "previousShared": null,
            "currentShared": "first-shared",
            "processEnvValue": "first-shared",
        })
    );

    let second_request = request("shared-worker-env:second", "second", "second-shared");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second shared-worker-env lease invocation should reuse cleanly");
    assert_eq!(
        second,
        serde_json::json!({
            "label": "second",
            "previousGlobal": null,
            "previousShared": null,
            "currentShared": "second-shared",
            "processEnvValue": "second-shared",
        }),
        "shared worker env helper state must be reseeded for each clean NodeFull lease realm"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_rebuilds_dynamic_module_map_per_realm() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dynamic-dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
let counter = 0;
globalThis.__nfr5DynamicModuleLoadCount =
  (globalThis.__nfr5DynamicModuleLoadCount ?? 0) + 1;

export const loadCount = globalThis.__nfr5DynamicModuleLoadCount;

export function next() {
  counter += 1;
  return counter;
}
"#,
    )
    .expect("dynamic dependency should write");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const previousMarker = globalThis.__nfr5DynamicImportMarker ?? null;
  const module = await import("./dynamic-dep.mjs");
  const first = module.next();
  const second = request.args.twice ? module.next() : null;
  globalThis.__nfr5DynamicImportMarker = request.args.label;
  return {
    label: request.args.label,
    previousMarker,
    loadCount: module.loadCount,
    first,
    second,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = |function_name: &str, label: &str, twice: bool| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: serde_json::json!({
            "label": label,
            "twice": twice,
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_request = request("module-map:first", "first", true);
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &first_request,
        &first_context,
    )
    .await
    .expect("first dynamic module-map lease invocation should return clean");
    assert_eq!(
        first,
        serde_json::json!({
            "label": "first",
            "previousMarker": null,
            "loadCount": 1,
            "first": 1,
            "second": 2,
        })
    );

    let second_request = request("module-map:second", "second", false);
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-a");
    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &second_request,
        &second_context,
    )
    .await
    .expect("second dynamic module-map lease invocation should reuse cleanly");
    assert_eq!(
        second,
        serde_json::json!({
            "label": "second",
            "previousMarker": null,
            "loadCount": 1,
            "first": 1,
            "second": null,
        }),
        "dynamic module-map entries and module-scoped state must be rebuilt per fresh lease realm"
    );
}

#[tokio::test]
async fn node_full_fresh_realm_lease_code_cache_reloads_changed_dependency_source() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "before";
}
"#,
    )
    .expect("dependency should write");
    std::fs::write(
        &bundle_path,
        r#"
import { value } from "./dep.mjs";

globalThis.__nimbusInvoke = function () {
  return { value: value() };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("NodeFull bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("NodeFull runtime should build from snapshot");
    let controller = RuntimeRealmLeaseController::new(Default::default());
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "code-cache:dependency".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let first = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &request,
        &context,
    )
    .await
    .expect("first code-cache lease invocation should succeed");
    assert_eq!(first, serde_json::json!({ "value": "before" }));
    let first_entry_count = bundle.module_code_cache_entry_count();
    let first_write_count = bundle.module_code_cache_write_count();
    assert_eq!(bundle.module_code_cache_partition_count(), 1);
    assert!(
        first_entry_count >= 2,
        "expected fresh-realm main module and dependency to populate cache"
    );

    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "after";
}
"#,
    )
    .expect("dependency update should write");

    let second = invoke_node_full_fresh_realm_with_lease(
        &runtime_owner,
        &controller,
        &mut runtime,
        &bundle,
        &request,
        &context,
    )
    .await
    .expect("second code-cache lease invocation should succeed");
    assert_eq!(
        second,
        serde_json::json!({ "value": "after" }),
        "fresh-realm module code cache must not serve stale dependency bytecode"
    );
    assert_eq!(
        bundle.module_code_cache_partition_count(),
        1,
        "source changes should stay inside the same strict realm authority partition"
    );
    assert_eq!(
        bundle.module_code_cache_entry_count(),
        first_entry_count,
        "changed dependency should replace its cache entry instead of adding a new partition"
    );
    assert!(
        bundle.module_code_cache_write_count() > first_write_count,
        "changed dependency source should compile and store fresh cached data"
    );
}

async fn invoke_node_full_fresh_realm_with_lease(
    runtime_owner: &NimbusRuntime,
    controller: &RuntimeRealmLeaseController,
    runtime: &mut JsRuntime,
    bundle: &RuntimeBundle,
    request: &InvocationRequest,
    context: &RuntimeInvocationContext,
) -> Result<Value> {
    let policy = runtime_owner.policy();
    let mut permit = SharedInvocationPermit::new(policy.clone(), None, None, true, None);
    permit.acquire_initial(std::time::Instant::now()).await?;
    let execution_plan = RuntimeExecutionPlan::for_invocation(policy.as_ref(), request, context);
    bootstrap::reset_runtime_invocation_state(
        runtime,
        permit.clone(),
        Some(context),
        Some(&execution_plan),
    );

    let (value, realm, mut lease) = runtime_owner
        .start_fresh_realm_bundle_invocation_with_lease_and_trace(
            controller,
            runtime,
            bundle,
            request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(context),
        )
        .await?;
    let response = runtime_owner
        .resolve_fresh_realm_invocation_response_with_lease_and_trace(
            runtime,
            &realm,
            value,
            bundle,
            request,
            V8RuntimeConstructionMode::StartupSnapshot,
            Some(context),
            &mut lease,
        )
        .await;
    let response = match response {
        Ok(response) => {
            if bootstrap::take_runtime_wait_until_pending(runtime) {
                match runtime_owner
                    .drain_wait_until_with_trace(
                        runtime,
                        Some(&realm),
                        Some(bundle),
                        request,
                        V8RuntimeConstructionMode::StartupSnapshot,
                        Some(context),
                    )
                    .await
                {
                    Ok(()) => Ok(response),
                    Err(error) => Err(error),
                }
            } else {
                Ok(response)
            }
        }
        Err(error) => Err(error),
    };
    super::super::realm_lifecycle::destroy_fresh_realm(runtime, realm);
    match response {
        Ok(response) => {
            runtime_owner.return_clean_fresh_realm_lease(runtime, &mut lease)?;
            assert!(
                permit.finish_invocation().await.is_empty(),
                "NodeFull fresh-realm lease helper should not leave ready jobs"
            );
            Ok(response)
        }
        Err(error) => {
            runtime_owner.condemn_dirty_fresh_realm_lease(&mut lease);
            assert!(
                permit.finish_invocation().await.is_empty(),
                "failed NodeFull fresh-realm lease helper should not leave ready jobs"
            );
            Err(error)
        }
    }
}

async fn invoke_node_full_fresh_realm_with_driver(
    runtime_owner: &NimbusRuntime,
    reusable_runtime: ReusableV8Runtime,
    bundle: &RuntimeBundle,
    request: &InvocationRequest,
    context: &RuntimeInvocationContext,
    cancellation: Option<HostCallCancellation>,
) -> (Result<Value>, Option<ReusableV8Runtime>) {
    let watchdog = WatchdogTimer::new();
    let policy = runtime_owner.policy();
    let mut permit = SharedInvocationPermit::new(
        policy.clone(),
        context.tenant_label.clone(),
        None,
        true,
        cancellation.clone(),
    );
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit NodeFull fresh-realm invocation");
    let execution_plan = RuntimeExecutionPlan::for_invocation(policy.as_ref(), request, context);
    let mut driver = runtime_owner
        .prepare_runtime_invocation_driver(
            reusable_runtime,
            watchdog.clone(),
            cancellation,
            permit.clone(),
            context,
            Some(&execution_plan),
            false,
        )
        .expect("driver preparation should install timeout and cancellation guards");

    let result = async {
        let lease_failure_reason = driver.realm_lease_condemnation_reason_classifier();
        let (value, realm, mut lease) = runtime_owner
            .start_fresh_realm_bundle_invocation_with_lease_and_reason_trace(
                &driver.realm_lease_controller,
                &mut driver.runtime,
                bundle,
                request,
                driver.construction_mode,
                Some(context),
                lease_failure_reason,
            )
            .await?;
        let response = runtime_owner
            .resolve_fresh_realm_invocation_response_with_lease_and_trace(
                &mut driver.runtime,
                &realm,
                value,
                bundle,
                request,
                driver.construction_mode,
                Some(context),
                &mut lease,
            )
            .await;
        let response = match response {
            Ok(response) => {
                if bootstrap::take_runtime_wait_until_pending(&mut driver.runtime) {
                    driver
                        .begin_wait_until_phase()
                        .await
                        .expect("waitUntil phase should arm cleanly");
                    match runtime_owner
                        .drain_wait_until_with_trace(
                            &mut driver.runtime,
                            Some(&realm),
                            Some(bundle),
                            request,
                            driver.construction_mode,
                            Some(context),
                        )
                        .await
                    {
                        Ok(()) => match driver.wait_until_phase_timeout_error() {
                            Some(error) => Err(error),
                            None => Ok(response),
                        },
                        Err(error) => Err(error),
                    }
                } else {
                    Ok(response)
                }
            }
            Err(error) => Err(error),
        };
        let destroy_started_at = std::time::Instant::now();
        super::super::realm_lifecycle::destroy_fresh_realm(&mut driver.runtime, realm);
        driver.record_fresh_realm_destroy(destroy_started_at.elapsed());
        match response {
            Ok(response) => {
                runtime_owner.return_clean_fresh_realm_lease(&mut driver.runtime, &mut lease)?;
                Ok(response)
            }
            Err(error) => {
                runtime_owner.condemn_fresh_realm_lease_with_reason(
                    &mut lease,
                    driver.realm_lease_condemnation_reason(),
                );
                Err(error)
            }
        }
    }
    .await;

    let (result, reusable_runtime) = driver.finalize_with_runtime(result).await;
    assert!(
        permit.finish_invocation().await.is_empty(),
        "NodeFull fresh-realm driver helper should not leave ready jobs"
    );
    watchdog.shutdown();
    (result, reusable_runtime)
}

#[tokio::test]
async fn node_full_fresh_realm_lease_host_pressure_eviction_preserves_authority_partition() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function (request) {
  return {
    functionName: request.function_name,
    tenantScopedMarker: globalThis.__tenantScopedMarker ?? null,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = crate::RuntimeLimits::application_node22();
    limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
    limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle;
    limits.node_full_realm_reuse_policy = RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_pool_entries_per_worker = 4;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner =
        NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
    let mut pool = V8WorkerRuntimePool::new();
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let tenant_a_request = request("messages:tenant-a");
    let tenant_a_context =
        RuntimeInvocationContext::top_level_for_tenant(&tenant_a_request, "tenant-a");
    let tenant_a_runtime = pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_a_context),
            true,
        )
        .expect("tenant-a should cold-miss into a fresh retained substrate");
    let (tenant_a_result, tenant_a_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        tenant_a_runtime,
        &bundle,
        &tenant_a_request,
        &tenant_a_context,
        None,
    )
    .await;
    assert_eq!(
        tenant_a_result.expect("tenant-a invocation should succeed"),
        serde_json::json!({
            "functionName": "messages:tenant-a",
            "tenantScopedMarker": null,
        })
    );
    pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_a_context),
        tenant_a_runtime.expect("tenant-a clean lease should return a reusable runtime"),
    );

    let tenant_b_request = request("messages:tenant-b");
    let tenant_b_context =
        RuntimeInvocationContext::top_level_for_tenant(&tenant_b_request, "tenant-b");
    let tenant_b_runtime = pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_b_context),
            true,
        )
        .expect("tenant-b should cold-miss into a separate retained substrate");
    let (tenant_b_result, tenant_b_runtime) = invoke_node_full_fresh_realm_with_driver(
        &runtime_owner,
        tenant_b_runtime,
        &bundle,
        &tenant_b_request,
        &tenant_b_context,
        None,
    )
    .await;
    assert_eq!(
        tenant_b_result.expect("tenant-b invocation should succeed"),
        serde_json::json!({
            "functionName": "messages:tenant-b",
            "tenantScopedMarker": null,
        })
    );
    pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_b_context),
        tenant_b_runtime.expect("tenant-b clean lease should return a reusable runtime"),
    );
    assert_eq!(pool.warm_pool_count_for_test(), 2);
    let node_full_maintenance = pool
        .last_boundary_maintenance_for_test()
        .expect("retained NodeFull realm substrate should record boundary maintenance");
    assert_eq!(
        node_full_maintenance.cleanliness.retained_memory_bytes,
        node_full_maintenance
            .heap_after
            .used_heap_size_bytes
            .saturating_add(node_full_maintenance.heap_after.external_memory_bytes),
        "NodeFull retained realm accounting must include V8-reported external memory"
    );
    let before_pressure = policy.metrics_snapshot();
    assert_eq!(before_pressure.warm_pool_misses, 2);
    assert_eq!(before_pressure.warm_pool_hits, 0);
    assert_eq!(before_pressure.retained_runtime_pool_entries, 2);

    let eviction = pool.apply_memory_pressure(&runtime_owner, RuntimeMemoryPressureLevel::High);
    assert_eq!(eviction.pressure, RuntimeMemoryPressureLevel::High);
    assert_eq!(eviction.evicted_entries, 1);
    assert_eq!(eviction.retained_entries, 1);
    let after_pressure = policy.metrics_snapshot();
    assert_eq!(after_pressure.retained_runtime_pool_evictions, 1);
    assert_eq!(after_pressure.retained_runtime_pool_entries, 1);

    let tenant_a_after_pressure = pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_a_context),
            true,
        )
        .expect("evicted tenant-a authority should build a fresh substrate");
    let after_tenant_a_checkout = policy.metrics_snapshot();
    assert_eq!(
        after_tenant_a_checkout.warm_pool_misses, 3,
        "tenant-a must cold-miss instead of taking tenant-b's retained substrate"
    );
    assert_eq!(after_tenant_a_checkout.warm_pool_hits, 0);
    assert_eq!(
        after_tenant_a_checkout.retained_runtime_pool_entries, 1,
        "tenant-b's retained substrate should remain in the pool"
    );
    let (tenant_a_after_pressure_result, tenant_a_after_pressure) =
        invoke_node_full_fresh_realm_with_driver(
            &runtime_owner,
            tenant_a_after_pressure,
            &bundle,
            &tenant_a_request,
            &tenant_a_context,
            None,
        )
        .await;
    assert_eq!(
        tenant_a_after_pressure_result.expect("tenant-a post-pressure invocation should succeed"),
        serde_json::json!({
            "functionName": "messages:tenant-a",
            "tenantScopedMarker": null,
        })
    );
    pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_a_context),
        tenant_a_after_pressure
            .expect("tenant-a post-pressure clean lease should return a reusable runtime"),
    );

    let tenant_b_after_pressure = pool
        .take_runtime_with_options_for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_b_context),
            true,
        )
        .expect("non-evicted tenant-b authority should reuse its retained substrate");
    let after_tenant_b_checkout = policy.metrics_snapshot();
    assert_eq!(
        after_tenant_b_checkout.warm_pool_hits, 1,
        "tenant-b should warm-hit because high pressure only evicted tenant-a"
    );
    assert_eq!(after_tenant_b_checkout.warm_pool_misses, 3);
    let (tenant_b_after_pressure_result, tenant_b_after_pressure) =
        invoke_node_full_fresh_realm_with_driver(
            &runtime_owner,
            tenant_b_after_pressure,
            &bundle,
            &tenant_b_request,
            &tenant_b_context,
            None,
        )
        .await;
    assert_eq!(
        tenant_b_after_pressure_result.expect("tenant-b post-pressure invocation should succeed"),
        serde_json::json!({
            "functionName": "messages:tenant-b",
            "tenantScopedMarker": null,
        })
    );
    pool.return_runtime_for_invocation(
        &runtime_owner,
        &bundle,
        Some(&tenant_b_context),
        tenant_b_after_pressure
            .expect("tenant-b post-pressure clean lease should return a reusable runtime"),
    );
}

#[tokio::test]
async fn warm_context_recycle_preserves_tenant_affinity_for_unscoped_bundle() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__entryLoadCount = (globalThis.__entryLoadCount ?? 0) + 1;

globalThis.__nimbusInvoke = async function (request) {
  return {
    entryLoadCount: globalThis.__entryLoadCount,
    functionName: request.function_name,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_pool_entries_per_worker = 4;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for tenant in ["tenant-a", "tenant-b", "tenant-a"] {
        let result = executor
            .invoke_on_worker(
                runtime.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&request, tenant),
                None,
            )
            .await
            .unwrap_or_else(|error| panic!("{tenant}: invocation should succeed: {error}"));
        assert_eq!(
            result,
            serde_json::json!({
                "entryLoadCount": 1,
                "functionName": "messages:list",
            }),
            "{tenant}: fresh-realm module state should not leak across retained runtime reuse"
        );
    }

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(
        metrics.runtime_pool_misses, 2,
        "tenant-b must cold-miss instead of reusing tenant-a's retained runtime"
    );
    assert_eq!(
        metrics.runtime_pool_hits, 1,
        "the final tenant-a invocation should reuse tenant-a's retained runtime"
    );
}

#[tokio::test]
async fn warm_context_recycle_preserves_function_affinity_for_unscoped_bundle() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__entryLoadCount = (globalThis.__entryLoadCount ?? 0) + 1;

globalThis.__nimbusInvoke = async function (request) {
  return {
    entryLoadCount: globalThis.__entryLoadCount,
    functionName: request.function_name,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.routing_affinity = RuntimeRoutingAffinity::Function;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_pool_entries_per_worker = 4;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = |function_name: &str| InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for function_name in ["messages:list", "messages:send", "messages:list"] {
        let request = request(function_name);
        let result = executor
            .invoke_on_worker(
                runtime.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a"),
                None,
            )
            .await
            .unwrap_or_else(|error| panic!("{function_name}: invocation should succeed: {error}"));
        assert_eq!(
            result,
            serde_json::json!({
                "entryLoadCount": 1,
                "functionName": function_name,
            }),
            "{function_name}: fresh-realm module state should be per-invocation"
        );
    }

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(
        metrics.runtime_pool_misses, 2,
        "messages:send must cold-miss instead of reusing messages:list's retained runtime"
    );
    assert_eq!(
        metrics.runtime_pool_hits, 1,
        "the final messages:list invocation should reuse the matching retained runtime"
    );
}

#[tokio::test]
async fn warm_context_recycle_preserves_script_affinity_for_distinct_bundle_entries() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_a_path = tempdir.path().join("bundle-a.mjs");
    std::fs::write(
        &bundle_a_path,
        r#"
globalThis.__entryLoadCount = (globalThis.__entryLoadCount ?? 0) + 1;
const scriptName = "bundle-a";

globalThis.__nimbusInvoke = async function (request) {
  return {
    entryLoadCount: globalThis.__entryLoadCount,
    scriptName,
    functionName: request.function_name,
  };
};

export {};
"#,
    )
    .expect("bundle-a should write");
    let bundle_b_path = tempdir.path().join("bundle-b.mjs");
    std::fs::write(
        &bundle_b_path,
        r#"
globalThis.__entryLoadCount = (globalThis.__entryLoadCount ?? 0) + 1;
const scriptName = "bundle-b";

globalThis.__nimbusInvoke = async function (request) {
  return {
    entryLoadCount: globalThis.__entryLoadCount,
    scriptName,
    functionName: request.function_name,
  };
};

export {};
"#,
    )
    .expect("bundle-b should write");

    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.routing_affinity = RuntimeRoutingAffinity::Script;
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_pool_entries_per_worker = 4;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle_a = RuntimeBundle::new(&bundle_a_path);
    let bundle_b = RuntimeBundle::new(&bundle_b_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for (bundle, expected_script) in [
        (&bundle_a, "bundle-a"),
        (&bundle_b, "bundle-b"),
        (&bundle_a, "bundle-a"),
    ] {
        let result = executor
            .invoke_on_worker(
                runtime.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a"),
                None,
            )
            .await
            .unwrap_or_else(|error| {
                panic!("{expected_script}: invocation should succeed: {error}")
            });
        assert_eq!(
            result,
            serde_json::json!({
                "entryLoadCount": 1,
                "scriptName": expected_script,
                "functionName": "messages:list",
            }),
            "{expected_script}: fresh-realm module state should be per-script invocation"
        );
    }

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(
        metrics.runtime_pool_misses, 2,
        "bundle-b must cold-miss instead of reusing bundle-a's retained runtime"
    );
    assert_eq!(
        metrics.runtime_pool_hits, 1,
        "the final bundle-a invocation should reuse the matching retained runtime"
    );
}

#[tokio::test]
async fn reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level(&request);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime(&runtime_owner, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    runtime_owner
        .load_bundle(&mut runtime, &bundle)
        .await
        .expect("bundle should load");

    let previous_cancel_handle = {
        let op_state = runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        cancellation_state.signal.cancel();
        assert!(
            cancellation_state.signal.is_cancelled(),
            "test should poison the previous invocation state"
        );
        cancellation_state.cancel_handle.clone()
    };

    let watchdog = WatchdogTimer::new();
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut driver = runtime_owner
        .prepare_runtime_invocation_driver(
            ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot),
            watchdog.clone(),
            None,
            permit.clone(),
            &context,
            None,
            false,
        )
        .expect("driver preparation should reset invocation state");

    {
        let op_state = driver.runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        assert!(
            !cancellation_state.signal.is_cancelled(),
            "fresh invocation state should not inherit the previous cancelled signal"
        );
        assert!(
            !Rc::ptr_eq(&previous_cancel_handle, &cancellation_state.cancel_handle),
            "fresh invocation state should replace the previous cancel handle"
        );
    }

    let result = runtime_owner
        .invoke_loaded_bundle(&mut driver.runtime, &request)
        .await
        .expect("fresh invocation state should allow async host work to complete");
    let result = driver
        .finalize(Ok(result))
        .await
        .expect("result should finalize");
    let ready_jobs = permit.finish_invocation().await;

    assert!(ready_jobs.is_empty());
    assert_eq!(
        result,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    watchdog.shutdown();
}

#[tokio::test]
async fn reused_runtime_uses_bound_host_call_session_before_next_invoke() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level(&request);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime(&runtime_owner, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, true, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    bootstrap::reset_runtime_invocation_state(&mut runtime, permit.clone(), Some(&context), None);

    async fn issue_default_context_get(runtime: &mut JsRuntime) -> Value {
        let value = runtime
            .execute_script(
                "<nimbus-runtime:test-default-context-get>",
                r#"(async () => {
  const ctx = globalThis.__nimbusCreateContext();
  return await ctx.db.get("messages", "doc-1");
})()"#,
            )
            .expect("test script should execute");
        let resolve = runtime.resolve(value);
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .expect("promise should resolve");
        deserialize_json_value(runtime, value).expect("result should deserialize")
    }

    let first = issue_default_context_get(&mut runtime).await;
    let second_without_reset = issue_default_context_get(&mut runtime).await;

    bootstrap::reset_bootstrap_invocation_state(&mut runtime)
        .expect("bootstrap reset should succeed on reused runtime");

    let third_after_reset = issue_default_context_get(&mut runtime).await;

    assert_eq!(
        first,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    assert_eq!(
        second_without_reset,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    assert_eq!(
        third_after_reset,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
}

#[tokio::test]
async fn fresh_realm_installs_bootstrap_and_uses_bound_host_bridge() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level(&request);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("bootstrap snapshot should build");
    let mut runtime = runtime_owner
        .create_runtime_from_snapshot(&bundle, snapshot)
        .expect("runtime should build from snapshot");
    let realm = runtime
        .create_realm(Default::default())
        .expect("fresh realm should be created inside retained runtime");

    let before_bootstrap = realm
        .execute_script(
            runtime.v8_isolate(),
            "<nimbus-runtime:test-fresh-realm-before-bootstrap>",
            "typeof globalThis.__nimbusCreateContext",
        )
        .expect("fresh realm pre-bootstrap probe should execute");
    assert_eq!(
        deserialize_json_value(&mut runtime, before_bootstrap)
            .expect("pre-bootstrap probe should deserialize"),
        serde_json::json!("undefined"),
        "fresh realm should not inherit Nimbus bootstrap globals from the main context"
    );

    bootstrap::install_bootstrap_in_realm(&mut runtime, &realm)
        .expect("Nimbus bootstrap should install into fresh realm");
    bootstrap::finalize_bootstrap_in_realm(&mut runtime, &realm)
        .expect("fresh realm bootstrap should finalize");
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, true, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit fresh-realm invocation");
    bootstrap::reset_runtime_invocation_state(&mut runtime, permit.clone(), Some(&context), None);
    bootstrap::reset_bootstrap_invocation_state_in_realm(&mut runtime, &realm)
        .expect("fresh realm bootstrap invocation state should reset");

    let realm_promise = realm
        .execute_script(
            runtime.v8_isolate(),
            "<nimbus-runtime:test-fresh-realm-host-call>",
            r#"(async () => {
  globalThis.__freshRealmMarker = "fresh-realm";
  const ctx = globalThis.__nimbusCreateContext({
    hostCallSessionId: "query:messages:get",
  });
  const response = await ctx.db.get("messages", "doc-1");
  return JSON.stringify({
    marker: globalThis.__freshRealmMarker,
    createContextType: typeof globalThis.__nimbusCreateContext,
    denoType: typeof globalThis.Deno,
    response,
  });
})()"#,
        )
        .expect("fresh realm host-call script should execute");
    let realm_result = runtime.resolve(realm_promise);
    let realm_result = runtime
        .with_event_loop_promise(realm_result, PollEventLoopOptions::default())
        .await
        .expect("fresh realm host-call promise should resolve");
    let realm_result = deserialize_json_value(&mut runtime, realm_result)
        .expect("fresh realm host-call result should deserialize");
    let realm_result: Value = serde_json::from_str(
        realm_result
            .as_str()
            .expect("fresh realm host-call result should be serialized JSON"),
    )
    .expect("fresh realm host-call result should parse");

    assert_eq!(
        realm_result,
        serde_json::json!({
            "marker": "fresh-realm",
            "createContextType": "function",
            "denoType": "undefined",
            "response": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "host_call_session_id": "query:messages:get",
                },
            },
        }),
        "fresh realm should run Nimbus bootstrap against the bound host bridge"
    );

    let main_context_probe = runtime
        .execute_script(
            "<nimbus-runtime:test-main-context-after-fresh-realm>",
            r#"JSON.stringify({
  marker: globalThis.__freshRealmMarker ?? null,
  createContextType: typeof globalThis.__nimbusCreateContext,
})"#,
        )
        .expect("main-context probe should execute");
    let main_context_probe = deserialize_json_value(&mut runtime, main_context_probe)
        .expect("main-context probe should deserialize");
    let main_context_probe: Value = serde_json::from_str(
        main_context_probe
            .as_str()
            .expect("main-context probe should be serialized JSON"),
    )
    .expect("main-context probe should parse");

    assert_eq!(
        main_context_probe,
        serde_json::json!({
            "marker": null,
            "createContextType": "function",
        }),
        "fresh realm globals must not pollute the retained runtime main context"
    );
    assert!(
        permit.finish_invocation().await.is_empty(),
        "fresh-realm test should not schedule ready jobs"
    );
}
