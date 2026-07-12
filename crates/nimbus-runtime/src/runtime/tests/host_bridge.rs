use crate::HostBridgeFuture;
use crate::limits::RuntimePolicy;

use super::*;

fn run_to_completion_policy_with_service_grant(service_name: &str) -> Arc<RuntimePolicy> {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.grants.service = vec![service_name.to_string()];
    Arc::new(RuntimePolicy::new(limits))
}

fn run_to_completion_policy_with_native_service_grant(service_name: &str) -> Arc<RuntimePolicy> {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.service_capability_enabled = true;
    limits.grants.service = vec![service_name.to_string()];
    Arc::new(RuntimePolicy::new(limits))
}

fn run_to_completion_policy_with_secret_and_identity_grants() -> Arc<RuntimePolicy> {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.grants.secret = vec!["stripe/live".to_string()];
    limits.grants.identity = vec!["service:agent-prod".to_string()];
    Arc::new(RuntimePolicy::new(limits))
}

#[tokio::test]
async fn runtime_async_ops_use_async_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const value = await ctx.db.get("messages", "doc-1");
  return { value };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(AsyncOnlyHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:get".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("async host bridge should satisfy async op");

    assert_eq!(result, serde_json::json!({ "value": "async-host" }));
}

#[tokio::test]
async fn runtime_query_context_is_reader_only_when_request_kind_is_present() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
// Declare this bundle's functions as same-lane so same-isolate nested ctx.run*
// takes local dispatch (this test asserts the local host-bridge path).
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(
    () => globalThis.__nimbusRuntimeEnvironmentLane,
  );
}
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  globalThis.__nimbusInvokeNamedLocal = async function (nestedRequest) {
    return {
      kind: nestedRequest.kind,
      functionName: nestedRequest.function_name,
      args: nestedRequest.args,
    };
  };
  const capture = async (fn) => {
    try {
      await fn();
      return null;
    } catch (error) {
      return String(error && error.message ? error.message : error);
    }
  };
  return {
    getType: typeof ctx.db.get,
    queryType: typeof ctx.db.query,
    runQueryResult: await ctx.runQuery(
      { name: "messages:list", visibility: "public" },
      { author: "Ada" },
    ),
    insertError: await capture(() => ctx.db.insert("messages", { body: "blocked" })),
    schedulerError: await capture(() => ctx.scheduler.runAfter(1, "messages:send", {})),
    runMutationError: await capture(() => ctx.runMutation("messages:send", {})),
    runActionError: await capture(() => ctx.runAction("messages:send", {})),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:reader".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("query context shape should be inspectable");

    assert_eq!(result["getType"], "function");
    assert_eq!(result["queryType"], "function");
    assert_eq!(
        result["runQueryResult"],
        serde_json::json!({
            "kind": "query",
            "functionName": "messages:list",
            "args": { "author": "Ada" },
        })
    );
    for field in [
        "insertError",
        "schedulerError",
        "runMutationError",
        "runActionError",
    ] {
        let message = result[field]
            .as_str()
            .expect("denied query context operation should return an error string");
        assert!(
            message.contains("not available for query handlers"),
            "unexpected {field}: {message}"
        );
    }
    let calls = host
        .calls
        .lock()
        .expect("host calls lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].operation,
        HostCallOperation::CtxRuntimeEnterNestedCall
    );
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "name": "messages:list",
            "visibility": "public",
            "kind": "query",
            "host_call_session_id": "query:messages:reader",
        })
    );
}

#[tokio::test]
async fn runtime_mutation_context_exposes_query_and_mutation_nested_calls() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
// Declare this bundle's functions as same-lane so same-isolate nested ctx.run*
// takes local dispatch (this test asserts the local host-bridge path).
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(
    () => globalThis.__nimbusRuntimeEnvironmentLane,
  );
}
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  globalThis.__nimbusInvokeNamedLocal = async function (nestedRequest) {
    return {
      kind: nestedRequest.kind,
      functionName: nestedRequest.function_name,
      args: nestedRequest.args,
    };
  };
  let runActionError = null;
  try {
    await ctx.runAction({ name: "messages:fanout", visibility: "public" }, {});
  } catch (error) {
    runActionError = String(error && error.message ? error.message : error);
  }
  return {
    schedulerType: typeof ctx.scheduler.runAfter,
    runQueryResult: await ctx.runQuery(
      { name: "messages:list", visibility: "public" },
      { author: "Ada" },
    ),
    runMutationResult: await ctx.runMutation(
      { name: "messages:send", visibility: "public" },
      { body: "hello" },
    ),
    runActionError,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: "messages:writer".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("mutation context shape should be inspectable");

    assert_eq!(result["schedulerType"], "function");
    assert_eq!(
        result["runQueryResult"],
        serde_json::json!({
            "kind": "query",
            "functionName": "messages:list",
            "args": { "author": "Ada" },
        })
    );
    assert_eq!(
        result["runMutationResult"],
        serde_json::json!({
            "kind": "mutation",
            "functionName": "messages:send",
            "args": { "body": "hello" },
        })
    );
    let run_action_error = result["runActionError"]
        .as_str()
        .expect("mutation runAction should return an error string");
    assert!(
        run_action_error.contains("not available for mutation handlers"),
        "unexpected mutation runAction denial: {run_action_error}"
    );
    let calls = host
        .calls
        .lock()
        .expect("host calls lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|call| call.operation == HostCallOperation::CtxRuntimeEnterNestedCall)
    );
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "name": "messages:list",
            "visibility": "public",
            "kind": "query",
            "host_call_session_id": "mutation:messages:writer",
        })
    );
    assert_eq!(
        calls[1].payload,
        serde_json::json!({
            "name": "messages:send",
            "visibility": "public",
            "kind": "mutation",
            "host_call_session_id": "mutation:messages:writer",
        })
    );
}

#[tokio::test]
async fn runtime_action_context_exposes_nested_calls_without_direct_db() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  let dbGetError = null;
  try {
    await ctx.db.get("messages", "doc-1");
  } catch (error) {
    dbGetError = String(error && error.message ? error.message : error);
  }
  return {
    dbGetError,
    schedulerType: typeof ctx.scheduler.runAfter,
    runQueryType: typeof ctx.runQuery,
    runMutationType: typeof ctx.runMutation,
    runActionType: typeof ctx.runAction,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "actions:inspect".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("action context shape should be inspectable");

    let db_get_error = result["dbGetError"]
        .as_str()
        .expect("action db access should return an error string");
    assert!(
        db_get_error.contains("not available for action handlers"),
        "unexpected action db denial: {db_get_error}"
    );
    assert_eq!(result["schedulerType"], "function");
    assert_eq!(result["runQueryType"], "function");
    assert_eq!(result["runMutationType"], "function");
    assert_eq!(result["runActionType"], "function");
}

#[tokio::test]
async fn runtime_exposes_verified_identity_extension_separately_from_convex_identity() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const request = arguments[0];
  const ctx = globalThis.__nimbusCreateContext({ request });
  return {
    user: await ctx.auth.getUserIdentity(),
    verified: await ctx.auth.getVerifiedIdentity(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "auth:whoami".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: Some(serde_json::json!({
                    "identity": {
                        "tokenIdentifier": "https://issuer.example.com|user-123",
                        "subject": "user-123",
                        "issuer": "https://issuer.example.com",
                        "email": "ada@example.com",
                        "given_name": "Ada",
                        "updated_at": 1710000000,
                        "address.formatted": "123 Analytical Engine Way",
                        "role": "admin"
                    },
                    "verified_identity": {
                        "kind": "custom_jwt",
                        "tokenIdentifier": "https://issuer.example.com|user-123",
                        "subject": "user-123",
                        "issuer": "https://issuer.example.com",
                        "name": "Ada Lovelace",
                        "givenName": "Ada",
                        "email": "ada@example.com",
                        "address": "123 Analytical Engine Way",
                        "updatedAt": "1710000000",
                        "role": "admin"
                    },
                    "throw_on_missing_identity": false,
                })),
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("runtime should expose both auth views");

    assert_eq!(
        result,
        serde_json::json!({
            "user": {
                "tokenIdentifier": "https://issuer.example.com|user-123",
                "subject": "user-123",
                "issuer": "https://issuer.example.com",
                "email": "ada@example.com",
                "given_name": "Ada",
                "updated_at": 1710000000,
                "address.formatted": "123 Analytical Engine Way",
                "role": "admin"
            },
            "verified": {
                "kind": "custom_jwt",
                "tokenIdentifier": "https://issuer.example.com|user-123",
                "subject": "user-123",
                "issuer": "https://issuer.example.com",
                "name": "Ada Lovelace",
                "givenName": "Ada",
                "email": "ada@example.com",
                "address": "123 Analytical Engine Way",
                "updatedAt": "1710000000",
                "role": "admin"
            }
        })
    );
}

#[tokio::test]
async fn runtime_secret_and_identity_grants_do_not_materialize_without_request_auth_or_secret_api()
{
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  return {
    user: await ctx.auth.getUserIdentity(),
    verified: await ctx.auth.getVerifiedIdentity(),
    contractGlobalType: typeof globalThis.__nimbusRuntimeContract,
    secretGlobalType: typeof globalThis.__nimbusSecrets,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_policy_with_secret_and_identity_grants(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "auth:whoami".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("runtime should execute without materializing grants");

    assert_eq!(
        result,
        serde_json::json!({
            "user": null,
            "verified": null,
            "contractGlobalType": "undefined",
            "secretGlobalType": "undefined",
        })
    );
}

#[tokio::test]
async fn adapter_context_omits_services_and_request_services() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  return {
    ctxServicesType: typeof ctx.services,
    hasCtxServices: Object.prototype.hasOwnProperty.call(ctx, "services"),
    requestServicesType: typeof request.services,
    requestKeys: Object.keys(request).sort(),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:describe".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: serde_json::from_value(serde_json::json!({
                    "db": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp",
                        "endpoints": {}
                    }
                }))
                .expect("service bindings should deserialize"),
            },
            "tenant-a",
        )
        .await
        .expect("runtime should execute adapter context without service shortcuts");

    assert_eq!(
        result,
        serde_json::json!({
            "ctxServicesType": "undefined",
            "hasCtxServices": false,
            "requestServicesType": "undefined",
            "requestKeys": ["args", "function_name", "kind"],
        }),
        "adapter ctx.services absent contract should omit both ctx.services and request.services"
    );
}

#[tokio::test]
async fn adapter_context_with_service_grant_still_has_no_raw_service_op() {
    let _guard = acquire_runtime_suite_lock().await;
    #[derive(Default)]
    struct ServiceLookupHost {
        async_calls: std::sync::Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for ServiceLookupHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            Err(NimbusRuntimeError::Contract(format!(
                "unexpected sync host op during adapter service-op absence test: {}",
                request.operation
            )))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            self.async_calls
                .lock()
                .expect("service lookup async host lock should not be poisoned")
                .push(request);
            Box::pin(async { Ok(serde_json::json!({ "status": "ok", "value": null })) })
        }
    }

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_service_lookup", {
    service_name: "db",
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(ServiceLookupHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_policy_with_service_grant("db"),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let error = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:denied".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect_err("adapter-created runtime must not register the service lookup op");

    assert!(
        error
            .to_string()
            .contains("Nimbus runtime async host op not found: op_nimbus_ctx_service_lookup"),
        "unexpected adapter service-op denial: {error}",
    );
    assert!(
        host.async_calls
            .lock()
            .expect("service lookup async host lock should not be poisoned")
            .is_empty(),
        "adapter service-op denial should not reach the host bridge",
    );
}

#[tokio::test]
async fn nimbus_native_service_op_uses_async_host_bridge_and_exact_grants() {
    let _guard = acquire_runtime_suite_lock().await;
    #[derive(Default)]
    struct ServiceLookupHost {
        sync_calls: std::sync::Mutex<Vec<HostCallRequest>>,
        async_calls: std::sync::Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for ServiceLookupHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.sync_calls
                .lock()
                .expect("service lookup sync host lock should not be poisoned")
                .push(request.clone());
            Err(NimbusRuntimeError::Contract(format!(
                "unexpected sync host op during native service lookup test: {}",
                request.operation
            )))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            self.async_calls
                .lock()
                .expect("service lookup async host lock should not be poisoned")
                .push(request.clone());
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "host": "127.0.0.1",
                        "port": 15432,
                        "protocol": "tcp",
                        "endpoints": {
                            "postgres": {
                                "host": "127.0.0.1",
                                "port": 15432,
                                "protocol": "tcp"
                            }
                        }
                    },
                }))
            })
        }
    }

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_service_lookup", {
    service_name: "db",
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(ServiceLookupHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_policy_with_native_service_grant("db"),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:get".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("native runtime should resolve service binding through the raw service op");

    assert_eq!(
        result,
        serde_json::json!({
            "host": "127.0.0.1",
            "port": 15432,
            "protocol": "tcp",
            "endpoints": {
                "postgres": {
                    "host": "127.0.0.1",
                    "port": 15432,
                    "protocol": "tcp"
                }
            }
        })
    );

    assert!(
        host.sync_calls
            .lock()
            .expect("service lookup sync host lock should not be poisoned")
            .is_empty(),
        "native service op should not use the sync host path"
    );
    let calls = host
        .async_calls
        .lock()
        .expect("service lookup async host lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 1, "missing service should be resolved once");
    assert_eq!(calls[0].operation, HostCallOperation::CtxServiceLookup);
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "service_name": "db",
            "host_call_session_id": "query:services:get",
        })
    );
}

#[tokio::test]
async fn nimbus_native_service_op_requires_exact_service_grant() {
    let _guard = acquire_runtime_suite_lock().await;
    #[derive(Default)]
    struct ServiceLookupHost {
        async_calls: std::sync::Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for ServiceLookupHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            Err(NimbusRuntimeError::Contract(format!(
                "unexpected sync host op during native service grant denial test: {}",
                request.operation
            )))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            self.async_calls
                .lock()
                .expect("service lookup async host lock should not be poisoned")
                .push(request);
            Box::pin(async { Ok(serde_json::json!({ "status": "ok", "value": null })) })
        }
    }

    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_service_lookup", {
    service_name: "cache",
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(ServiceLookupHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_policy_with_native_service_grant("db"),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let error = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "services:denied".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect_err("native runtime should require an exact service grant");

    assert!(
        error
            .to_string()
            .contains("runtime service grant denied for `cache`"),
        "unexpected native service grant denial: {error}",
    );
    assert!(
        host.async_calls
            .lock()
            .expect("service lookup async host lock should not be poisoned")
            .is_empty(),
        "denied service lookup should not reach the host bridge",
    );
}

#[tokio::test]
async fn runtime_query_builder_setup_uses_sync_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const builder = ctx
    .db
    .query("messages")
    .withIndex("by_author", (q) => q.eq(q.field("author"), "Ada"))
    .filter((q) => q.eq(q.field("channel"), "general"))
    .order("desc");
  return { builderId: builder.__builderId };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(SyncOnlyHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("sync host bridge should satisfy query builder setup");

    assert_eq!(result, serde_json::json!({ "builderId": "builder-1" }));
    let calls = host
        .calls
        .lock()
        .expect("sync-only host lock should not be poisoned")
        .clone();
    assert_eq!(
        calls
            .into_iter()
            .map(|call| call.operation)
            .collect::<Vec<_>>(),
        vec![
            HostCallOperation::QueryBuilderStart,
            HostCallOperation::QueryBuilderWithIndex,
            HostCallOperation::QueryBuilderFilter,
            HostCallOperation::QueryBuilderOrder,
        ]
    );
}

#[tokio::test]
async fn runtime_async_write_and_scheduler_ops_use_async_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const insert = await ctx.db.insert("messages", { body: "hello" });
  const patch = await ctx.db.patch("messages", "doc-1", { body: "updated" });
  const deletion = await ctx.db.delete("messages", "doc-1");
  const runAfter = await ctx.scheduler.runAfter(
    100,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled" },
  );
  const runAt = await ctx.scheduler.runAt(
    500,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled-at" },
  );
  const cancel = await ctx.scheduler.cancel("job-1");
  return { insert, patch, deletion, runAfter, runAt, cancel };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: "messages:write".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("async host bridge should satisfy write and scheduler ops");

    assert_eq!(
        result,
        serde_json::json!({
            "insert": {
                "operation": "document_insert",
                "payload": {
                    "table": "messages",
                    "fields": { "body": "hello" },
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "patch": {
                "operation": "document_patch",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "patch": { "body": "updated" },
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "deletion": {
                "operation": "document_delete",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "runAfter": {
                "operation": "ctx_scheduler_run_after",
                "payload": {
                    "delay_ms": 100,
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "scheduled" },
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "runAt": {
                "operation": "ctx_scheduler_run_at",
                "payload": {
                    "timestamp_ms": 500,
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "scheduled-at" },
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "cancel": {
                "operation": "ctx_scheduler_cancel",
                "payload": {
                    "job_id": "job-1",
                    "host_call_session_id": "mutation:messages:write",
                }
            }
        })
    );
}

#[tokio::test]
async fn runtime_db_ops_accept_single_table_scoped_id_convention() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const get = await ctx.db.get("messages:doc-1");
  const patch = await ctx.db.patch("messages:doc-1", { body: "updated" });
  const deletion = await ctx.db.delete("messages:doc-1");
  return { get, patch, deletion };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: "messages:write".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("single table-scoped id db ops should reach the host bridge");

    assert_eq!(
        result,
        serde_json::json!({
            "get": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "messages:doc-1",
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "patch": {
                "operation": "document_patch",
                "payload": {
                    "table": "messages",
                    "id": "messages:doc-1",
                    "patch": { "body": "updated" },
                    "host_call_session_id": "mutation:messages:write",
                }
            },
            "deletion": {
                "operation": "document_delete",
                "payload": {
                    "table": "messages",
                    "id": "messages:doc-1",
                    "host_call_session_id": "mutation:messages:write",
                }
            }
        })
    );
}

#[tokio::test]
async fn runtime_db_single_id_ops_reject_ids_without_table_scope() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const capture = async (fn) => {
    try {
      await fn();
      return null;
    } catch (error) {
      return String(error && error.message ? error.message : error);
    }
  };
  return {
    getError: await capture(() => ctx.db.get("doc-1")),
    getNonStringError: await capture(() => ctx.db.get(42)),
    patchError: await capture(() => ctx.db.patch("doc-1", { body: "updated" })),
    deleteError: await capture(() => ctx.db.delete(":doc-1")),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Mutation,
                function_name: "messages:write".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("malformed single-id db calls should fail in the contract shim");

    assert_eq!(
        result["getError"],
        "ctx.db.get(...) requires a table-scoped document id like \"tasks:...\", got \"doc-1\""
    );
    assert_eq!(
        result["getNonStringError"],
        "ctx.db.get(...) requires a table-scoped document id string"
    );
    assert_eq!(
        result["patchError"],
        "ctx.db.patch(...) requires a table-scoped document id like \"tasks:...\", got \"doc-1\""
    );
    assert_eq!(
        result["deleteError"],
        "ctx.db.delete(...) requires a table-scoped document id like \"tasks:...\", got \":doc-1\""
    );
    let calls = host
        .calls
        .lock()
        .expect("recording host lock should not be poisoned")
        .clone();
    assert!(
        calls.is_empty(),
        "malformed ids must never reach the host bridge, got {calls:?}"
    );
}

#[tokio::test]
async fn runtime_extension_call_uses_async_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return await globalThis.__nimbusAsyncHostValue(
    "op_nimbus_runtime_extension_call",
    {
      namespace: "cloud_functions",
      operation: "firebase_admin.firestore.get",
      payload: { path: "messages/doc-1" },
    },
  );
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "extensions:call".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("async host bridge should satisfy runtime extension calls");

    assert_eq!(
        result,
        serde_json::json!({
            "operation": "runtime_extension_call",
            "payload": {
                "namespace": "cloud_functions",
                "operation": "firebase_admin.firestore.get",
                "payload": { "path": "messages/doc-1" },
            }
        })
    );
}

#[tokio::test]
async fn runtime_query_paginate_uses_async_host_bridge_and_returns_official_shape() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 2,
    cursor: null,
    maximumRowsRead: 32,
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(PaginateHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:listPage".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("paginate query should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "page": [
                { "body": "hello" }
            ],
            "isDone": true,
            "continueCursor": "",
            "splitCursor": null,
            "pageStatus": null,
        })
    );

    let sync_calls = host
        .sync_calls
        .lock()
        .expect("paginate host sync lock should not be poisoned")
        .clone();
    assert_eq!(sync_calls.len(), 1);
    assert_eq!(
        sync_calls[0].operation,
        HostCallOperation::QueryBuilderStart
    );

    let async_calls = host
        .async_calls
        .lock()
        .expect("paginate host async lock should not be poisoned")
        .clone();
    assert_eq!(async_calls.len(), 1);
    assert_eq!(
        async_calls[0].operation,
        HostCallOperation::QueryReadPaginate
    );
    assert_eq!(
        async_calls[0].payload,
        serde_json::json!({
            "builder_id": "builder-1",
            "page_size": 2,
            "cursor": Value::Null,
            "host_call_session_id": "query:messages:listPage",
        })
    );
}

#[tokio::test]
async fn runtime_query_paginate_treats_full_page_with_cursor_as_not_done() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 1,
    cursor: "after-alpha",
  });
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(PaginateContinuationHost);
    let runtime = NimbusRuntime::with_policy(
        host,
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:listPage".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("paginate query should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "page": [
                { "body": "beta" }
            ],
            "isDone": false,
            "continueCursor": "after-beta",
            "splitCursor": null,
            "pageStatus": null,
        })
    );
}

#[tokio::test]
async fn runtime_same_isolate_nested_entry_uses_sync_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
// Declare this bundle's functions as same-lane so the nested ctx.run* takes
// local dispatch (this test asserts the sync host-bridge path it uses).
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(
    () => globalThis.__nimbusRuntimeEnvironmentLane,
  );
}
globalThis.__nimbusInvokeNamedLocal = async function () {
  return "local-ok";
};

globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  return await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(SyncOnlyHost::default());
    let runtime = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:outer".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("same-isolate nested entry should succeed");

    assert_eq!(result, serde_json::json!("local-ok"));
    let calls = host
        .calls
        .lock()
        .expect("sync-only host lock should not be poisoned")
        .clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].operation,
        HostCallOperation::CtxRuntimeEnterNestedCall
    );
    assert_eq!(
        calls[0].payload,
        serde_json::json!({
            "name": "messages:list",
            "visibility": "public",
            "kind": "query",
            "host_call_session_id": "query:messages:outer",
        })
    );
}

#[tokio::test]
async fn runtime_async_ctx_run_ops_use_async_host_bridge_path() {
    let _guard = acquire_runtime_suite_lock().await;
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  const query = await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
  const mutation = await ctx.runMutation(
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "hello" },
  );
  const action = await ctx.runAction(
    { name: "messages:sendViaAction", visibility: "public" },
    { body: "wave" },
  );
  return { query, mutation, action };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "messages:outer".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            "tenant-a",
        )
        .await
        .expect("async host bridge should satisfy ctx.run* fallback ops");

    assert_eq!(
        result,
        serde_json::json!({
            "query": {
                "operation": "ctx_run_query",
                "payload": {
                    "name": "messages:list",
                    "visibility": "public",
                    "args": { "author": "Ada" },
                    "host_call_session_id": "action:messages:outer",
                }
            },
            "mutation": {
                "operation": "ctx_run_mutation",
                "payload": {
                    "name": "messages:storeInternal",
                    "visibility": "internal",
                    "args": { "body": "hello" },
                    "host_call_session_id": "action:messages:outer",
                }
            },
            "action": {
                "operation": "ctx_run_action",
                "payload": {
                    "name": "messages:sendViaAction",
                    "visibility": "public",
                    "args": { "body": "wave" },
                    "host_call_session_id": "action:messages:outer",
                }
            }
        })
    );
}
